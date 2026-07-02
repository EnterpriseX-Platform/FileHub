//! AI-native HTTP endpoints (read side):
//!   * `POST /fh/api/search/semantic` — find files by meaning (pgvector kNN),
//!     permission-scoped exactly like keyword search.
//!   * `GET  /fh/api/files/:id/ai`    — per-file AI panel (summary/tags/…) +
//!     enrichment status.
//!
//!   * `POST /fh/api/ask`            — RAG: a grounded, **cited**, permission-
//!     aware answer over the caller's accessible content.
//!
//! Writes (embeddings, summaries) are produced asynchronously by `ai_worker`.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai;
use crate::auth::{effective_system_ids, ensure_system_access, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SemanticQuery {
    pub q: String,
    pub limit: Option<i64>,
    pub system_id: Option<String>,
}

#[derive(Serialize)]
pub struct SemanticHit {
    pub file_id: Uuid,
    pub name: String,
    pub file_type: String,
    pub system_id: String,
    pub snippet: String,
    pub seq: i32,
    /// Cosine similarity in [-1, 1] (1 = closest). Derived from pgvector `<=>`.
    pub score: f64,
}

const SNIPPET_CHARS: usize = 280;

/// POST /fh/api/search/semantic — meaning-based search over chunk embeddings.
/// Returns the best-matching chunk per file, newest-best first, scoped to the
/// systems the caller may read (same authorization as keyword search).
pub async fn semantic_search(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(q): Json<SemanticQuery>,
) -> ApiResult<Json<Vec<SemanticHit>>> {
    if !s.ai.enabled() {
        return Err(ApiError::BadRequest("AI is disabled (AI_ENABLED=false)".into()));
    }
    let term = q.q.trim();
    if term.is_empty() {
        return Err(ApiError::BadRequest("q must not be empty".into()));
    }
    let limit = q.limit.unwrap_or(10).clamp(1, 50);

    // Same authorization as keyword search: optional explicit system must be
    // visible; otherwise scope to the caller's effective systems.
    if let Some(ref sid) = q.system_id {
        ensure_system_access(&s.db, &user.0, sid).await?;
    }
    let scope = effective_system_ids(&s.db, &user.0).await?;

    // Embed the query, then kNN. We over-fetch chunks and dedupe to the best
    // chunk per file in Rust (simpler than DISTINCT ON + re-sort in SQL).
    let (qvecs, usage) = s.ai.embed(std::slice::from_ref(&term.to_string())).await?;
    ai::record_usage(&s.db, Some(&user.0.id), q.system_id.as_deref(), "embed", s.ai.embed_model(), &usage).await;
    let qvec = qvecs
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("no embedding returned for query")))?;
    let qlit = ai::vector_literal(&qvec);
    let overfetch = (limit * 5).min(200);

    let mut sql = String::from(
        "SELECT f.id, f.name, f.file_type, f.system_id, c.content, c.seq, \
                (ce.embedding <=> $1::vector) AS dist \
         FROM chunk_embeddings ce \
         JOIN chunks c ON c.id = ce.chunk_id \
         JOIN files  f ON f.id = c.file_id \
         WHERE f.deleted_at IS NULL",
    );
    let mut n = 1usize;
    if q.system_id.is_some() {
        n += 1;
        sql.push_str(&format!(" AND f.system_id = ${n}"));
    }
    let scope_vec = scope.clone();
    if scope_vec.is_some() {
        n += 1;
        sql.push_str(&format!(" AND f.system_id = ANY(${n})"));
    }
    n += 1;
    sql.push_str(&format!(" ORDER BY dist ASC LIMIT ${n}"));

    let mut query =
        sqlx::query_as::<_, (Uuid, String, String, String, String, i32, f64)>(&sql).bind(&qlit);
    if let Some(ref sid) = q.system_id {
        query = query.bind(sid);
    }
    if let Some(ref sc) = scope_vec {
        query = query.bind(sc);
    }
    query = query.bind(overfetch);

    let rows = query.fetch_all(&s.db).await?;

    // Dedupe to the best (lowest-distance) chunk per file, preserving order.
    let mut out: Vec<SemanticHit> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (file_id, name, file_type, system_id, content, seq, dist) in rows {
        if !seen.insert(file_id) {
            continue;
        }
        out.push(SemanticHit {
            file_id,
            name,
            file_type,
            system_id,
            snippet: snippet(&content),
            seq,
            score: 1.0 - dist,
        });
        if out.len() as i64 >= limit {
            break;
        }
    }
    Ok(Json(out))
}

fn snippet(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= SNIPPET_CHARS {
        return trimmed.to_string();
    }
    let s: String = trimmed.chars().take(SNIPPET_CHARS).collect();
    format!("{s}…")
}

#[derive(Serialize, sqlx::FromRow)]
pub struct FileAi {
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub language: Option<String>,
    pub sensitivity: String,
    pub model: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct FileAiResponse {
    /// Enrichment lifecycle for the UI: none | queued | running | done | failed.
    pub status: String,
    pub ai: Option<FileAi>,
}

/// GET /fh/api/files/:id/ai — the per-file AI panel data + enrichment status.
/// 404 (not 403) when the caller can't see the file, so personal-drive
/// existence doesn't leak via status code — same rule as the rest of the API.
pub async fn file_ai(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FileAiResponse>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&s.db)
            .await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    if ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }

    let ai_row: Option<FileAi> = sqlx::query_as::<_, FileAi>(
        "SELECT summary, tags, language, sensitivity, model, updated_at FROM file_ai WHERE file_id = $1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await?;

    // Latest job status (for a "analyzing…" spinner vs "no AI yet").
    let status: Option<(String,)> = sqlx::query_as(
        "SELECT status FROM ai_jobs WHERE file_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await?;
    let status = status.map(|(st,)| st).unwrap_or_else(|| "none".into());

    Ok(Json(FileAiResponse { status, ai: ai_row }))
}

// ---- RAG: ask your content -------------------------------------------------

#[derive(Deserialize)]
pub struct AskQuery {
    pub question: String,
    /// Optional scoping. file_id pins the answer to one document; system_id to
    /// one system. Absent → the caller's whole accessible corpus.
    pub system_id: Option<String>,
    pub file_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct Citation {
    pub n: usize,
    pub file_id: Uuid,
    pub name: String,
    pub system_id: String,
    pub snippet: String,
    pub score: f64,
}

#[derive(Serialize)]
pub struct AskResponse {
    pub answer: String,
    pub citations: Vec<Citation>,
}

// Per-chunk cap on context handed to the model. Must comfortably exceed the
// chunker's typical chunk size (~1.2–2k chars) or answers living in the back
// half of a chunk are silently truncated away; 700 did exactly that.
// Worst case context: k(≤10) files × MAX_CHUNKS_PER_FILE(2) × 1600 chars.
const ASK_CHUNK_CHARS: usize = 1600;

/// POST /fh/api/ask — retrieve permission-scoped chunks, ground the model on
/// them, return a cited answer. The security invariant: retrieval is filtered
/// by the **caller's current** effective_system_ids, so a user can never get an
/// answer drawn from content they can't read.
pub async fn ask(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(q): Json<AskQuery>,
) -> ApiResult<Json<AskResponse>> {
    if !s.ai.enabled() {
        return Err(ApiError::BadRequest("AI is disabled (AI_ENABLED=false)".into()));
    }
    let question = q.question.trim().to_string();
    if question.is_empty() {
        return Err(ApiError::BadRequest("question must not be empty".into()));
    }
    let k = q.limit.unwrap_or(6).clamp(1, 10);

    if let Some(ref sid) = q.system_id {
        ensure_system_access(&s.db, &user.0, sid).await?;
    }
    let scope = effective_system_ids(&s.db, &user.0).await?;

    // 1) embed the question, 2) retrieve the best chunk per file (permission-scoped).
    let (qvecs, embed_usage) = s.ai.embed(std::slice::from_ref(&question)).await?;
    ai::record_usage(&s.db, Some(&user.0.id), q.system_id.as_deref(), "embed", s.ai.embed_model(), &embed_usage).await;
    let qvec = qvecs
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::Other(anyhow::anyhow!("no embedding returned for question")))?;
    let qlit = ai::vector_literal(&qvec);
    let overfetch = (k * 5).min(60);

    let mut sql = String::from(
        "SELECT f.id, f.name, f.system_id, c.content, (ce.embedding <=> $1::vector) AS dist \
         FROM chunk_embeddings ce \
         JOIN chunks c ON c.id = ce.chunk_id \
         JOIN files  f ON f.id = c.file_id \
         WHERE f.deleted_at IS NULL",
    );
    let mut n = 1usize;
    if q.file_id.is_some() {
        n += 1;
        sql.push_str(&format!(" AND f.id = ${n}"));
    }
    if q.system_id.is_some() {
        n += 1;
        sql.push_str(&format!(" AND f.system_id = ${n}"));
    }
    if scope.is_some() {
        n += 1;
        sql.push_str(&format!(" AND f.system_id = ANY(${n})"));
    }
    n += 1;
    sql.push_str(&format!(" ORDER BY dist ASC LIMIT ${n}"));

    let mut query = sqlx::query_as::<_, (Uuid, String, String, String, f64)>(&sql).bind(&qlit);
    if let Some(fid) = q.file_id {
        query = query.bind(fid);
    }
    if let Some(ref sid) = q.system_id {
        query = query.bind(sid);
    }
    if let Some(ref sc) = scope {
        query = query.bind(sc);
    }
    query = query.bind(overfetch);
    let rows = query.fetch_all(&s.db).await?;

    // Group chunks per file (up to MAX_CHUNKS_PER_FILE each), file order by
    // best-chunk distance (rows arrive sorted by distance asc). One numbered
    // source per FILE — a doc's additional relevant chunks are concatenated
    // into the same source instead of being discarded, so an answer living in
    // a later section isn't lost to the doc's own header chunk.
    const MAX_CHUNKS_PER_FILE: usize = 2;
    let mut cand: Vec<(Uuid, String, String, Vec<String>, f64)> = Vec::new();
    for (file_id, name, system_id, content, dist) in rows {
        match cand.iter_mut().find(|c| c.0 == file_id) {
            Some(c) => {
                if c.3.len() < MAX_CHUNKS_PER_FILE {
                    c.3.push(content);
                }
            }
            None => cand.push((file_id, name, system_id, vec![content], 1.0 - dist)),
        }
    }
    // Relevance gate — drop weak matches that would only mislead the model
    // (e.g. an unrelated doc that happens to be the next-nearest neighbour).
    // Keep sources at/above an absolute floor AND close to the top score.
    const REL_FLOOR: f64 = 0.4;
    let top = cand.first().map(|c| c.4).unwrap_or(0.0);
    let kept: Vec<_> = cand
        .into_iter()
        .filter(|c| c.4 >= REL_FLOOR && c.4 >= top - 0.2)
        .take(k as usize)
        .collect();

    if kept.is_empty() {
        return Ok(Json(AskResponse {
            answer: "I couldn't find anything in the content you can access to answer that.".into(),
            citations: vec![],
        }));
    }

    let mut citations: Vec<Citation> = Vec::new();
    let mut contexts: Vec<String> = Vec::new();
    for (file_id, name, system_id, chunks, score) in kept {
        let num = citations.len() + 1;
        let body: String = chunks
            .iter()
            .map(|c| c.trim().chars().take(ASK_CHUNK_CHARS).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n…\n");
        contexts.push(format!("[{num}] {name}\n{body}"));
        citations.push(Citation { n: num, file_id, name, system_id, snippet: snippet(&chunks[0]), score });
    }

    // 3) ground the model on the numbered sources.
    let context = contexts.join("\n\n");
    let system = "You answer questions strictly from the provided numbered sources. \
        Cite the sources you use inline with their bracket number, like [1]. \
        If the sources do not contain the answer, say you don't have that information. \
        Be concise and do not invent facts.";
    let prompt = format!("Question: {question}\n\nSources:\n{context}");
    let (answer, chat_usage) = s.ai.chat(system, &prompt).await?;
    ai::record_usage(&s.db, Some(&user.0.id), q.system_id.as_deref(), "ask", s.ai.chat_model(), &chat_usage).await;

    Ok(Json(AskResponse { answer: answer.trim().to_string(), citations }))
}

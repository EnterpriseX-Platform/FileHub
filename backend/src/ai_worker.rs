//! AI enrichment worker — drains the `ai_jobs` queue and makes files
//! *understood*: extract text → chunk → embed (pgvector) → summarise + tag.
//!
//! Modeled on `rotation.rs`: a single background task on an interval, spawned
//! from `main.rs`, awaited on graceful shutdown. Best-effort and idempotent —
//! a failing job is marked `failed` (and retried on the next tick), never
//! crashes the process, and never blocks an upload (uploads only *enqueue*).
//!
//! Enrichment is gated on `AiClient::enabled()`; with `AI_ENABLED=false` the
//! worker isn't spawned and FileHub behaves exactly as before.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::ai;
use crate::AppState;

/// Char window per chunk and overlap between adjacent chunks. ~2000 chars is
/// roughly 500 tokens; the overlap preserves context across boundaries so a
/// fact split across two chunks is still retrievable. Token-aware chunking can
/// replace this later (spec §8 D-CHUNK) without touching the schema.
const CHUNK_CHARS: usize = 2000;
const CHUNK_OVERLAP: usize = 300;
/// Cap chunks per file so a giant document can't dominate a tick.
const MAX_CHUNKS: usize = 400;
/// How many characters of a document we feed the summariser.
const SUMMARY_INPUT_CHARS: usize = 6000;
/// Jobs processed per tick.
const JOBS_PER_TICK: usize = 8;

/// Enqueue an enrichment job for a freshly-uploaded (or re-versioned) file.
/// Cheap INSERT; the worker does the heavy lifting. Best-effort — a failure to
/// enqueue is logged, never surfaced to the uploader.
pub async fn enqueue(db: &sqlx::PgPool, file_id: Uuid) {
    let res = sqlx::query("INSERT INTO ai_jobs (id, file_id, kind) VALUES ($1, $2, 'enrich')")
        .bind(Uuid::now_v7())
        .bind(file_id)
        .execute(db)
        .await;
    if let Err(e) = res {
        tracing::warn!("ai enqueue for {file_id} failed: {e:#}");
    }
}

/// Spawn the worker loop. Mirrors `rotation::spawn_worker`.
pub fn spawn_worker(state: Arc<AppState>, interval: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tracing::info!("ai enrichment worker started (interval {:?})", interval);
        loop {
            tick.tick().await;
            for _ in 0..JOBS_PER_TICK {
                match process_next(&state).await {
                    Ok(false) => break,            // queue empty
                    Ok(true) => {}                 // processed one; try the next
                    Err(e) => {
                        tracing::warn!("ai worker tick error: {e:#}");
                        break;
                    }
                }
            }
        }
    })
}

/// Claim and process the next pending job. Returns Ok(false) when the queue is
/// empty. A job-level failure is recorded on the row (not returned) so the loop
/// keeps draining; only infrastructure errors (DB unreachable) propagate.
async fn process_next(state: &AppState) -> anyhow::Result<bool> {
    // Atomically claim the oldest queued/failed job. SKIP LOCKED keeps this
    // safe if we ever run more than one worker.
    let claimed: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"UPDATE ai_jobs SET status='running', attempts=attempts+1, updated_at=now()
           WHERE id = (
               SELECT id FROM ai_jobs
               WHERE status IN ('queued','failed') AND attempts < 5
               ORDER BY created_at
               FOR UPDATE SKIP LOCKED
               LIMIT 1
           )
           RETURNING id, file_id"#,
    )
    .fetch_optional(&state.db)
    .await?;

    let (job_id, file_id) = match claimed {
        Some(j) => j,
        None => return Ok(false),
    };

    match enrich_file(state, file_id).await {
        Ok(()) => {
            sqlx::query("UPDATE ai_jobs SET status='done', last_error=NULL, updated_at=now() WHERE id=$1")
                .bind(job_id)
                .execute(&state.db)
                .await?;
        }
        Err(e) => {
            tracing::warn!("enrich file {file_id} failed: {e:#}");
            sqlx::query("UPDATE ai_jobs SET status='failed', last_error=$2, updated_at=now() WHERE id=$1")
                .bind(job_id)
                .bind(format!("{e:#}"))
                .execute(&state.db)
                .await?;
        }
    }
    Ok(true)
}

/// Text already indexed for this file (e.g. OCR output written to file_content).
async fn load_indexed_content(state: &AppState, file_id: Uuid) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT content FROM file_content WHERE file_id = $1")
        .bind(file_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

/// The enrichment pipeline for one file: load → extract → chunk → embed →
/// summarise. Each step is guarded so a partial failure leaves a coherent state.
async fn enrich_file(state: &AppState, file_id: Uuid) -> anyhow::Result<()> {
    // 1) Load the file's storage coordinates.
    let row: Option<(String, bool, String, String, String)> = sqlx::query_as(
        "SELECT object_key, encrypted, file_type, name, system_id FROM files WHERE id=$1 AND deleted_at IS NULL",
    )
    .bind(file_id)
    .fetch_optional(&state.db)
    .await?;
    let (object_key, encrypted, file_type, name, system_id) = match row {
        Some(r) => r,
        None => return Ok(()), // file deleted before we got to it — nothing to do
    };

    // 2) Re-read the (decrypted) bytes from storage and extract text.
    let bytes = match state.storage.get(&object_key, encrypted).await? {
        Some((b, _)) => b,
        None => return Ok(()),
    };
    let text = match crate::p1::extract_text_from(&file_type, &bytes) {
        Some(t) if !t.trim().is_empty() => t,
        _ => {
            // No directly-extractable text (scanned image / image-only PDF).
            // Fall back to any OCR'd text already indexed for this file
            // (ocr.rs writes it to file_content). Only if that's empty too do we
            // record a minimal "analyzed, no text" row.
            match load_indexed_content(state, file_id).await {
                Some(t) if !t.trim().is_empty() => t,
                _ => {
                    upsert_file_ai(state, file_id, None, &[], None, "none").await?;
                    return Ok(());
                }
            }
        }
    };

    // 3) Chunk, embed, and (re)write the vector index for this file.
    let chunks = chunk_text(&text);
    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let (embeddings, embed_usage) = state.ai.embed(&texts).await?;
    ai::record_usage(&state.db, None, Some(&system_id), "embed", state.ai.embed_model(), &embed_usage).await;
    if embeddings.len() != chunks.len() {
        anyhow::bail!("embeddings/chunks length mismatch");
    }

    let mut tx = state.db.begin().await?;
    // Idempotent: drop any prior chunks for this file (CASCADE clears embeddings).
    sqlx::query("DELETE FROM chunks WHERE file_id=$1")
        .bind(file_id)
        .execute(&mut *tx)
        .await?;
    let model = state.ai.embed_model().to_string();
    for (c, emb) in chunks.iter().zip(embeddings.iter()) {
        let chunk_id = Uuid::now_v7();
        sqlx::query(
            r#"INSERT INTO chunks (id, file_id, seq, content, char_start, char_end, token_count)
               VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
        )
        .bind(chunk_id)
        .bind(file_id)
        .bind(c.seq)
        .bind(&c.content)
        .bind(c.char_start)
        .bind(c.char_end)
        .bind((c.content.len() / 4) as i32)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO chunk_embeddings (chunk_id, embedding, model) VALUES ($1, $2::vector, $3)",
        )
        .bind(chunk_id)
        .bind(ai::vector_literal(emb))
        .bind(&model)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    // 4) Summarise + tag (best-effort — embeddings already landed, so a chat
    //    failure here doesn't lose the searchable index).
    let (summary, tags, language, sensitivity) = match summarise(state, &system_id, &name, &text).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("summarise {file_id} failed (index still written): {e:#}");
            return Ok(());
        }
    };
    upsert_file_ai(state, file_id, summary.as_deref(), &tags, language.as_deref(), &sensitivity).await?;
    Ok(())
}

struct Chunk {
    seq: i32,
    content: String,
    char_start: i32,
    char_end: i32,
}

/// Sliding-window char chunker with overlap. Operates on char boundaries so it
/// never splits a UTF-8 codepoint.
fn chunk_text(text: &str) -> Vec<Chunk> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut seq = 0i32;
    let step = CHUNK_CHARS.saturating_sub(CHUNK_OVERLAP).max(1);
    while start < chars.len() && out.len() < MAX_CHUNKS {
        let end = (start + CHUNK_CHARS).min(chars.len());
        let content: String = chars[start..end].iter().collect();
        if !content.trim().is_empty() {
            out.push(Chunk {
                seq,
                content,
                char_start: start as i32,
                char_end: end as i32,
            });
            seq += 1;
        }
        if end >= chars.len() {
            break;
        }
        start += step;
    }
    out
}

#[derive(Deserialize, Default)]
struct Analysis {
    summary: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    language: Option<String>,
    sensitivity: Option<String>,
}

/// Ask the chat model for a structured summary. Tolerant parser: pulls the
/// first {...} block out of the response in case the model wraps it in prose.
async fn summarise(
    state: &AppState,
    system_id: &str,
    name: &str,
    text: &str,
) -> anyhow::Result<(Option<String>, Vec<String>, Option<String>, String)> {
    let head: String = text.chars().take(SUMMARY_INPUT_CHARS).collect();
    let system = "You are a precise document analyst. Respond ONLY with a single compact JSON object, no prose, no code fences.";
    let user = format!(
        "Analyze this document titled \"{name}\". Return JSON with keys: \
         summary (<=3 sentences), tags (array of up to 6 short lowercase topic strings), \
         language (ISO 639-1 code), sensitivity (one of \"none\", \"pii\", \"confidential\"). \
         Document:\n\n{head}"
    );
    let (raw, usage) = state.ai.chat(system, &user).await?;
    ai::record_usage(&state.db, None, Some(system_id), "summarize", state.ai.chat_model(), &usage).await;
    let json_slice = match (raw.find('{'), raw.rfind('}')) {
        (Some(a), Some(b)) if b > a => &raw[a..=b],
        _ => raw.trim(),
    };
    let a: Analysis = serde_json::from_str(json_slice).unwrap_or_else(|_| Analysis {
        // Model didn't return valid JSON — keep the raw text as the summary so
        // the work isn't wasted.
        summary: Some(raw.trim().chars().take(800).collect()),
        ..Default::default()
    });
    let sensitivity = match a.sensitivity.as_deref() {
        Some("pii") => "pii",
        Some("confidential") => "confidential",
        _ => "none",
    }
    .to_string();
    let tags: Vec<String> = a.tags.into_iter().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).take(6).collect();
    Ok((a.summary, tags, a.language, sensitivity))
}

async fn upsert_file_ai(
    state: &AppState,
    file_id: Uuid,
    summary: Option<&str>,
    tags: &[String],
    language: Option<&str>,
    sensitivity: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO file_ai (file_id, summary, tags, language, sensitivity, model, updated_at)
           VALUES ($1,$2,$3,$4,$5,$6, now())
           ON CONFLICT (file_id) DO UPDATE SET
             summary=EXCLUDED.summary, tags=EXCLUDED.tags, language=EXCLUDED.language,
             sensitivity=EXCLUDED.sensitivity, model=EXCLUDED.model, updated_at=now()"#,
    )
    .bind(file_id)
    .bind(summary)
    .bind(tags)
    .bind(language)
    .bind(sensitivity)
    .bind(state.ai.chat_model())
    .execute(&state.db)
    .await?;
    Ok(())
}

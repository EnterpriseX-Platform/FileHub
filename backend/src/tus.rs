//! TUS 1.0.0 resumable upload (https://tus.io/protocols/resumable-upload).
//!
//! Implements `creation` + `termination` extensions:
//!
//!   POST   /fh/api/uploads              create
//!   HEAD   /fh/api/uploads/:id          progress
//!   PATCH  /fh/api/uploads/:id          append chunk
//!   DELETE /fh/api/uploads/:id          terminate
//!
//! When the running offset reaches `Upload-Length`, the temp blob is read
//! back, pushed through the storage layer (encrypt + ObjectStore put), a
//! `files` row is inserted, and the preview/thumbnail pipeline runs the same
//! as a regular upload.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use serde::Serialize;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::handlers::detect_file_type;
use crate::state::AppState;

pub const TUS_VERSION: &str = "1.0.0";

fn tus_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(HeaderName::from_static("tus-resumable"), HeaderValue::from_static(TUS_VERSION));
    h.insert(HeaderName::from_static("tus-version"),   HeaderValue::from_static(TUS_VERSION));
    h.insert(HeaderName::from_static("tus-extension"), HeaderValue::from_static("creation,termination"));
    h
}

/// Where TUS partial blobs live before they're moved to the real storage.
fn tus_root() -> std::path::PathBuf {
    let root = std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./storage".into());
    std::path::PathBuf::from(root).join(".tus")
}

/// Decode the comma-separated `Upload-Metadata` header — each entry is
/// `key value-base64`.  Values are base64-encoded so they can carry binary
/// or UTF-8 cleanly.
fn parse_upload_metadata(raw: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for entry in raw.split(',') {
        let mut it = entry.trim().splitn(2, ' ');
        let key = match it.next() { Some(k) if !k.is_empty() => k, _ => continue };
        let val = match it.next() {
            Some(v) => B64.decode(v.trim()).ok()
                        .and_then(|b| String::from_utf8(b).ok())
                        .unwrap_or_default(),
            None    => String::new(),
        };
        out.insert(key.to_string(), val);
    }
    out
}

/// Public dump returned to clients for debugging.
#[derive(Debug, Serialize)]
pub struct TusInfo {
    pub id:            Uuid,
    pub upload_length: i64,
    pub upload_offset: i64,
    pub filename:      Option<String>,
    pub expires_at:    chrono::DateTime<Utc>,
    pub completed_at:  Option<chrono::DateTime<Utc>>,
}

// -----------------------------------------------------------------------------
// POST /fh/api/uploads — create a new session.
// -----------------------------------------------------------------------------
pub async fn create_session(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    headers: HeaderMap,
) -> ApiResult<Response> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    crate::auth::upload_rate_limit(&user.0.id).await?;
    let upload_length: i64 = headers
        .get("upload-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ApiError::BadRequest("Upload-Length required".into()))?;
    if upload_length < 0 {
        return Err(ApiError::BadRequest("Upload-Length must be non-negative".into()));
    }

    let meta_raw = headers.get("upload-metadata").and_then(|v| v.to_str().ok()).unwrap_or("");
    let meta = parse_upload_metadata(meta_raw);
    // tus-js-client encodes the whole metadata object, including keys whose
    // values are empty strings (the Upload page sends `org_id: orgId || ""`
    // even when no org is picked).  Treat empty strings as "absent" so we
    // don't try to bind `org_id=""` into a TEXT FK column — Postgres rejects
    // that with a foreign-key violation and the whole upload appears to
    // "hang" client-side.  Applies uniformly to every optional metadata
    // field to keep the rules consistent.
    let pick = |k: &str| meta.get(k).filter(|v| !v.is_empty()).cloned();

    // Verify destination access early — the same system_id is bound into the
    // tus_uploads row, and finalise() will read it back when stitching the
    // chunks into the real file row.  Rejecting here means a viewer can't
    // even open a TUS session against a personal drive they don't own.
    let system_id_meta = pick("system_id")
        .ok_or_else(|| ApiError::BadRequest("system_id required in upload metadata".into()))?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id_meta).await?;

    let id = Uuid::now_v7();
    let temp_key = format!("{id}.part");
    let dir = tus_root();
    fs::create_dir_all(&dir).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    fs::File::create(dir.join(&temp_key)).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;

    sqlx::query(
        r#"INSERT INTO tus_uploads
            (id, user_id, upload_length, upload_offset, filename, content_type,
             system_id, org_id, folder_id, project, status_meta, owner_meta, tags_meta, temp_key)
           VALUES ($1,$2,$3,0,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)"#,
    )
    .bind(id)
    .bind(&user.0.id)
    .bind(upload_length)
    .bind(pick("filename"))
    .bind(pick("content_type"))
    .bind(&system_id_meta)
    .bind(pick("org_id"))
    .bind(pick("folder_id"))
    .bind(pick("project"))
    .bind(pick("status"))
    .bind(pick("owner"))
    .bind(pick("tags"))
    .bind(&temp_key)
    .execute(&s.db).await?;

    // Per TUS spec the Location header MAY be relative; tus-js-client resolves
    // it against the endpoint URL it POSTed to with `new URL(location, endpoint)`.
    //
    // We used to emit an absolute path `/fh/api/uploads/{id}`.  That works
    // when the browser talks to the backend directly, but when the request
    // arrives through the Next.js proxy (`/filehub/api/uploads` → backend
    // `/fh/api/uploads`) the resolved URL becomes `http://host/fh/api/...`,
    // which the proxy doesn't rewrite — PATCH chunks hit Next's 404 handler
    // and the upload silently hangs forever.
    //
    // The relative form `uploads/{id}` resolves correctly both ways:
    //   browser → http://host:3001/filehub/api/uploads/<id>  (through proxy)
    //   curl    → http://host:8091/fh/api/uploads/<id>       (direct to backend)
    let location = format!("uploads/{id}");
    let mut h = tus_headers();
    h.insert(HeaderName::from_static("location"), HeaderValue::from_str(&location).unwrap());
    Ok((StatusCode::CREATED, h).into_response())
}

// -----------------------------------------------------------------------------
// HEAD /fh/api/uploads/:id — report current Upload-Offset.
// -----------------------------------------------------------------------------
pub async fn head_session(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let row: Option<(String, i64, i64)> = sqlx::query_as(
        "SELECT COALESCE(user_id,''), upload_length, upload_offset FROM tus_uploads WHERE id = $1"
    ).bind(id).fetch_optional(&s.db).await?;
    let (owner, length, offset) = row.ok_or(ApiError::NotFound)?;
    if owner != user.0.id { return Err(ApiError::Forbidden); }

    let mut h = tus_headers();
    h.insert(HeaderName::from_static("upload-offset"), length_header(offset));
    h.insert(HeaderName::from_static("upload-length"), length_header(length));
    h.insert(HeaderName::from_static("cache-control"), HeaderValue::from_static("no-store"));
    Ok((StatusCode::OK, h).into_response())
}

fn length_header(n: i64) -> HeaderValue {
    HeaderValue::from_str(&n.to_string()).unwrap()
}

// -----------------------------------------------------------------------------
// PATCH /fh/api/uploads/:id — append a chunk.
// -----------------------------------------------------------------------------
pub async fn append_chunk(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let row: Option<(String, i64, i64, String)> = sqlx::query_as(
        "SELECT COALESCE(user_id,''), upload_length, upload_offset, temp_key FROM tus_uploads WHERE id = $1"
    ).bind(id).fetch_optional(&s.db).await?;
    let (owner, length, current_offset, temp_key) = row.ok_or(ApiError::NotFound)?;
    if owner != user.0.id { return Err(ApiError::Forbidden); }

    // TUS requires Upload-Offset and Content-Type: application/offset+octet-stream
    let client_offset: i64 = headers.get("upload-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ApiError::BadRequest("Upload-Offset required".into()))?;
    if client_offset != current_offset {
        // 409 conflict per spec; we map to 400 since ApiError doesn't model 409 yet.
        return Err(ApiError::BadRequest(format!(
            "Upload-Offset mismatch: server has {current_offset}, client sent {client_offset}"
        )));
    }
    let ct = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
    if ct != "application/offset+octet-stream" {
        return Err(ApiError::BadRequest("Content-Type must be application/offset+octet-stream".into()));
    }

    let chunk_len = body.len() as i64;
    if current_offset + chunk_len > length {
        return Err(ApiError::BadRequest("chunk exceeds Upload-Length".into()));
    }

    let path = tus_root().join(&temp_key);
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .await
        .map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    f.write_all(&body).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    f.flush().await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;

    let new_offset = current_offset + chunk_len;
    sqlx::query("UPDATE tus_uploads SET upload_offset = $1 WHERE id = $2")
        .bind(new_offset).bind(id).execute(&s.db).await?;

    let finalised = new_offset == length;
    if finalised {
        finalise(&s, id, &temp_key, &user.0.id).await?;
    }

    let mut h = tus_headers();
    h.insert(HeaderName::from_static("upload-offset"), length_header(new_offset));
    Ok((StatusCode::NO_CONTENT, h).into_response())
}

/// Move the temp blob to the regular storage layer, create the file row, and
/// hand off to the same downstream pipeline (text index + thumbnail + Office
/// preview).
async fn finalise(s: &AppState, tus_id: Uuid, temp_key: &str, user_id: &str) -> Result<(), ApiError> {
    let path = tus_root().join(temp_key);
    let body = fs::read(&path).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;

    // Pull the saved metadata back out.
    let row: (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        r#"SELECT filename, content_type, system_id, org_id, folder_id, project, status_meta, owner_meta, tags_meta
             FROM tus_uploads WHERE id = $1"#,
    )
    .bind(tus_id).fetch_one(&s.db).await?;
    let (filename, content_type, system_id, org_id, folder_id, project, status_meta, owner_meta, tags_meta) = row;

    let name = filename.unwrap_or_else(|| format!("upload-{tus_id}.bin"));
    let system_id = system_id.ok_or_else(|| ApiError::BadRequest("missing system_id in upload metadata".into()))?;

    let system: crate::models::System = sqlx::query_as("SELECT * FROM systems WHERE id = $1")
        .bind(&system_id).fetch_optional(&s.db).await?
        .ok_or_else(|| ApiError::BadRequest("unknown system_id in upload metadata".into()))?;

    let new_file_id = Uuid::now_v7();
    let file_type = detect_file_type(&name);
    // See handlers::persist_upload — original name kept in DB, but the path
    // segment is rewritten through sanitize_filename so a malicious TUS
    // session can't traverse or inject quotes into the storage key.
    let safe = crate::handlers::sanitize_filename(&name);
    let object_key = format!("{}/{}-{}", system.bucket, new_file_id, safe);
    let size_bytes = body.len() as i64;

    // Enforce nested workspace/system/org/user quota before committing the
    // bytes to storage.  TUS clients may have streamed gigabytes by now, but
    // if the assembled file blows past the cap we'd rather refuse here than
    // leak the storage.
    crate::handlers::enforce_quota(s, size_bytes, Some(&system_id), org_id.as_deref(), Some(user_id)).await?;

    let ct = content_type.clone().or_else(|| mime_guess::from_path(&name).first().map(|m| m.to_string()));
    let body_bytes = bytes::Bytes::from(body);
    let etag = s.storage.put(&object_key, body_bytes.clone(), ct.as_deref()).await.map_err(ApiError::Other)?;
    let encrypted = s.storage.encryption_enabled();
    let owner = owner_meta.unwrap_or_else(|| "Anonymous".into());
    let status = status_meta.unwrap_or_else(|| "Draft".into());
    let tags_json = tags_meta.unwrap_or_else(|| "[]".into());
    let now = Utc::now();

    sqlx::query(
        r#"INSERT INTO files
            (id, name, file_type, size_bytes, system_id, org_id, folder_id, bucket, object_key,
             project, status, owner, tags, version, metadata, etag, encrypted, created_at, modified_at, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,1,'{}',$14,$15,$16,$16,$17)"#,
    )
    .bind(new_file_id).bind(&name).bind(&file_type).bind(size_bytes)
    .bind(&system_id).bind(&org_id).bind(&folder_id)
    .bind(&system.bucket).bind(&object_key)
    .bind(&project).bind(&status).bind(&owner).bind(&tags_json)
    .bind(&etag).bind(encrypted).bind(now).bind(user_id)
    .execute(&s.db).await?;

    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,$3,'uploaded',$4,$5,$6,$7,$8,$9,$10)"#,
    )
    .bind(Uuid::now_v7()).bind(&owner).bind(&system.tone)
    .bind(&name).bind(&file_type).bind(new_file_id)
    .bind(&system_id).bind(&org_id).bind(now).bind(user_id)
    .execute(&s.db).await?;

    sqlx::query("UPDATE tus_uploads SET file_id = $1, completed_at = $2 WHERE id = $3")
        .bind(new_file_id).bind(now).bind(tus_id)
        .execute(&s.db).await?;

    // Downstream P1 pipelines.
    if let Some(text) = crate::p1::extract_text_from(&file_type, &body_bytes) {
        crate::p1::index_file_content(&s.db, new_file_id, &text).await;
    }
    crate::p1::index_thumbnail(&s.db, new_file_id, &file_type, &body_bytes).await;
    crate::p1::index_office_preview(&s.db, new_file_id, &file_type, &name, &body_bytes).await;

    // AI-native: enqueue understanding for the resumable-upload path too.
    if s.ai.enabled() {
        crate::ai_worker::enqueue(&s.db, new_file_id).await;
    }

    let _ = fs::remove_file(&path).await;
    Ok(())
}

// -----------------------------------------------------------------------------
// DELETE /fh/api/uploads/:id — terminate.
// -----------------------------------------------------------------------------
pub async fn terminate_session(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT COALESCE(user_id,''), temp_key FROM tus_uploads WHERE id = $1 AND completed_at IS NULL"
    ).bind(id).fetch_optional(&s.db).await?;
    let (owner, temp_key) = row.ok_or(ApiError::NotFound)?;
    if owner != user.0.id { return Err(ApiError::Forbidden); }

    sqlx::query("DELETE FROM tus_uploads WHERE id = $1").bind(id).execute(&s.db).await?;
    let _ = fs::remove_file(tus_root().join(&temp_key)).await;
    let mut h = tus_headers();
    h.insert(HeaderName::from_static("cache-control"), HeaderValue::from_static("no-store"));
    Ok((StatusCode::NO_CONTENT, h).into_response())
}

// -----------------------------------------------------------------------------
// OPTIONS /fh/api/uploads — capabilities (TUS-Resumable handshake).
// -----------------------------------------------------------------------------
pub async fn options_capabilities() -> Response {
    let mut h = tus_headers();
    h.insert(HeaderName::from_static("tus-max-size"), HeaderValue::from_static("0"));  // 0 = unlimited
    (StatusCode::NO_CONTENT, h).into_response()
}

// =============================================================================
// Unit tests — header parsing.
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upload_metadata_pairs() {
        let raw = "filename ZmlsZS50eHQ=,system_id c3lzX2hy,tags Wyl0ZXN0Il0=";
        let m = parse_upload_metadata(raw);
        assert_eq!(m["filename"],  "file.txt");
        assert_eq!(m["system_id"], "sys_hr");
    }

    #[test]
    fn parses_empty_value() {
        let m = parse_upload_metadata("flag,filename ZmlsZS50eHQ=");
        assert_eq!(m.get("flag").map(String::as_str), Some(""));
        assert_eq!(m["filename"], "file.txt");
    }
}

//! Import files from the old file service (legacy FileService API) into FileHub.
//!
//! ## Why
//! A legacy file service can hold tens of thousands of files that other
//! applications reference by a `FILE_ID` stored in their own databases. If the
//! IDs changed during migration, every application's data would have to be
//! rewritten, which is impractical.
//!
//! Key point: this importer **keeps every original file ID** (both sides use UUIDs)
//! ⇒ migrated files open with their old IDs immediately, no application data is
//! touched, and once everything is imported the old service can really be
//! retired (no more request forwarding).
//!
//! ## Idempotent
//! Files that already exist are skipped (counted as skipped) ⇒ re-run to pick up
//! the remainder. Safe to run while the system is live, since the old service is
//! only read, never modified.
//!
//! ## Deliberately no thumbnails / text extraction during import
//! Tens of thousands of files would overload the machine at once — do that
//! later, gradually, as a separate background job.

use std::sync::Arc;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::handlers::sanitize_filename;
use crate::models::System;
use crate::AppState;

#[derive(Deserialize)]
pub struct ImportRequest {
    /// Number of files to scan per run (default 50 · max 500).
    pub limit: Option<i64>,
    /// Start offset in the old service's file list.
    pub offset: Option<i64>,
    /// Explicit file IDs (for picking up files that were missed).
    pub ids: Option<Vec<String>>,
    /// Report only; write nothing.
    pub dry_run: Option<bool>,
}

#[derive(Serialize, Default)]
pub struct ImportResult {
    pub scanned: usize,
    pub imported: usize,
    pub skipped_existing: usize,
    pub failed: usize,
    pub bytes: i64,
    pub errors: Vec<String>,
    pub dry_run: bool,
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

async fn legacy_token(client: &reqwest::Client) -> Option<String> {
    let url = env_opt("LEGACY_TOKEN_URL")?;
    let v: serde_json::Value = client.get(&url).send().await.ok()?.json().await.ok()?;
    v.get("token").and_then(|t| t.as_str()).map(|s| s.to_string())
}

fn s(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

/// Import files from the old file service — admin only.
pub async fn import_from_legacy(
    State(st): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<ImportRequest>,
) -> ApiResult<Json<ImportResult>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let base = env_opt("LEGACY_FILEHUB_URL")
        .ok_or_else(|| ApiError::BadRequest("LEGACY_FILEHUB_URL is not set".into()))?;
    let system_id = env_opt("LEGACY_DEFAULT_SYSTEM")
        .or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"))
        .ok_or_else(|| ApiError::BadRequest("LEGACY_DEFAULT_SYSTEM is not set".into()))?;
    let system: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1")
        .bind(&system_id)
        .fetch_optional(&st.db)
        .await?
        .ok_or_else(|| ApiError::BadRequest("unknown target system".into()))?;

    let dry = req.dry_run.unwrap_or(false);
    let limit = req.limit.unwrap_or(50).clamp(1, 500);
    let offset = req.offset.unwrap_or(0).max(0);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    let token = legacy_token(&client).await;

    // Files to import: explicitly listed, or fetched page by page from the old service.
    let mut rows: Vec<serde_json::Value> = Vec::new();
    if let Some(ids) = req.ids.clone() {
        for id in ids {
            let url = format!("{base}/FileService/getFileDetail?fileId={id}");
            let mut r = client.get(&url);
            if let Some(t) = &token { r = r.bearer_auth(t); }
            match r.send().await {
                Ok(resp) => match resp.json::<serde_json::Value>().await {
                    Ok(v) => { if let Some(d) = v.get("data") { rows.push(d.clone()); } }
                    Err(e) => return Err(ApiError::Other(anyhow::anyhow!("cannot read file detail: {e}"))),
                },
                Err(e) => return Err(ApiError::Other(anyhow::anyhow!("cannot reach the old file service: {e}"))),
            }
        }
    } else {
        let url = format!("{base}/FileService/getFiles?offset={offset}&limitOfset={limit}");
        let mut r = client.get(&url);
        if let Some(t) = &token { r = r.bearer_auth(t); }
        let v: serde_json::Value = r
            .send()
            .await
            .map_err(|e| ApiError::Other(anyhow::anyhow!("cannot reach the old file service: {e}")))?
            .json()
            .await
            .map_err(|e| ApiError::Other(anyhow::anyhow!("cannot read file list: {e}")))?;
        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            rows = arr.clone();
        }
    }

    let mut out = ImportResult { dry_run: dry, ..Default::default() };
    for row in rows {
        out.scanned += 1;
        // Legacy folders are not files — skip.
        if s(&row, "type_").as_deref() == Some("DIRECTORY") {
            continue;
        }
        let Some(raw_id) = s(&row, "file_system_id").or_else(|| s(&row, "id")) else {
            out.failed += 1;
            out.errors.push("row has no file ID".into());
            continue;
        };
        let Ok(id) = Uuid::parse_str(&raw_id) else {
            out.failed += 1;
            out.errors.push(format!("ID is not a UUID: {raw_id}"));
            continue;
        };
        let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM files WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
        if exists.is_some() {
            out.skipped_existing += 1;
            continue;
        }
        let name = s(&row, "full_name_type")
            .or_else(|| s(&row, "fileName"))
            .or_else(|| s(&row, "file_name"))
            .unwrap_or_else(|| raw_id.clone());
        if dry {
            out.imported += 1;
            continue;
        }

        // Fetch the bytes from the old service.
        let url = format!("{base}/FileService/downloadFile?fileId={raw_id}&logType=download");
        let mut rq = client.get(&url);
        if let Some(t) = &token { rq = rq.bearer_auth(t); }
        let resp = match rq.send().await {
            Ok(r) => r,
            Err(e) => { out.failed += 1; out.errors.push(format!("{name}: download failed {e}")); continue; }
        };
        if !resp.status().is_success() {
            out.failed += 1;
            out.errors.push(format!("{name}: old service answered {}", resp.status()));
            continue;
        }
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or(v).trim().to_string());
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => { out.failed += 1; out.errors.push(format!("{name}: cannot read bytes {e}")); continue; }
        };

        let safe = sanitize_filename(&name);
        let object_key = format!("{}/{}-{}", system.bucket, id, safe);
        let etag = match st.storage.put(&object_key, body.clone(), ct.as_deref()).await {
            Ok(e) => e,
            Err(e) => { out.failed += 1; out.errors.push(format!("{name}: write failed {e}")); continue; }
        };
        let encrypted = st.storage.encryption_enabled();
        let file_type = crate::handlers::detect_file_type(&name);
        let size = body.len() as i64;

        // A provenance tag + the original tags if any, so imported files can be traced back.
        let mut tags: Vec<String> = vec!["via:legacy-import".into()];
        if let Some(t) = s(&row, "tag_name") {
            if let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(&t) {
                for x in a { if let Some(v) = x.as_str() { tags.push(v.to_string()); } }
            } else {
                tags.push(t);
            }
        }
        // The old service stores the owner as a raw user ID; copied as-is the UI would
        // show a long unreadable UUID — keep the original ID as a tag for traceability
        // and show a human-readable owner instead.
        let owner = "Imported from legacy hub".to_string();
        if let Some(uid) = s(&row, "owner_user_id") {
            tags.push(format!("legacy-user:{uid}"));
        }

        let res = sqlx::query(
            r#"INSERT INTO files
                (id, name, file_type, size_bytes, system_id, org_id, folder_id, bucket, object_key,
                 project, status, owner, tags, version, metadata, etag, encrypted, created_at, modified_at, created_by)
               VALUES ($1,$2,$3,$4,$5,NULL,NULL,$6,$7,NULL,'Archived',$8,$9,1,'{}',$10,$11,now(),now(),$12)"#,
        )
        .bind(id)
        .bind(&name)
        .bind(&file_type)
        .bind(size)
        .bind(&system_id)
        .bind(&system.bucket)
        .bind(&object_key)
        .bind(&owner)
        .bind(serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()))
        .bind(&etag)
        .bind(encrypted)
        .bind(&user.0.id)
        .execute(&st.db)
        .await;
        match res {
            Ok(_) => { out.imported += 1; out.bytes += size; }
            Err(e) => {
                out.failed += 1;
                out.errors.push(format!("{name}: failed to save {e}"));
                let _ = st.storage.delete(&object_key).await;
            }
        }
        if out.errors.len() > 20 { out.errors.truncate(20); }
    }
    tracing::info!(
        scanned = out.scanned, imported = out.imported, skipped = out.skipped_existing,
        failed = out.failed, "imported files from the old file service"
    );
    Ok(Json(out))
}

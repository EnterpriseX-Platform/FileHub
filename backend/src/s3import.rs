//! Import a whole bucket from MinIO (or any S3 service) into a basket — the
//! `mc mirror` of moving to FileHub.
//!
//! Each object goes through the same path as an S3 PUT to FileHub: its key
//! becomes folders + file name, and the basket's rules, access and default lock
//! apply. Re-running skips objects already imported with the same size, so an
//! interrupted import just continues. Progress is kept in `s3_import_jobs`.

use std::sync::Arc;

use axum::{extract::{Path, State}, Json};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use crate::Id;

use crate::auth::{AdminUser, User};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::s3::S3Store;

#[derive(Deserialize)]
pub struct Source {
    endpoint: String,
    #[serde(default = "us_east_1")]
    region: String,
    bucket: String,
    #[serde(default)]
    prefix: String,
    access_key: String,
    secret_key: String,
    /// Destination basket.
    system_id: String,
}
fn us_east_1() -> String { "us-east-1".into() }

/// POST /api/admin/import-s3 — start an import (admin); returns the job id.
pub async fn start(State(s): State<Arc<AppState>>, admin: AdminUser, Json(src): Json<Source>) -> ApiResult<Json<Value>> {
    if !crate::s3::enabled() { return Err(ApiError::NotFound); }
    let (bucket,): (String,) = sqlx::query_as("SELECT bucket FROM systems WHERE id = $1 AND deleted_at IS NULL")
        .bind(&src.system_id).fetch_optional(&s.db).await?.ok_or_else(|| ApiError::BadRequest("Unknown basket.".into()))?;
    let url = reqwest::Url::parse(&src.endpoint).map_err(|_| ApiError::BadRequest("Can't use that S3 address.".into()))?;
    if !crate::webhooks::outbound_allowed(&url).await { return Err(ApiError::BadRequest("FileHub doesn't call that address.".into())); }
    let store = S3Store::new(&src.endpoint, &src.region, &src.bucket, &src.access_key, &src.secret_key)
        .map_err(|e| ApiError::BadRequest(format!("Can't use that S3 address: {e}")))?;
    // Fail fast on a wrong address/key instead of a job that fails at once.
    store.list_page(&src.prefix, None).await.map_err(|e| { tracing::warn!("s3 import: list failed: {e}"); ApiError::BadRequest("Can't list the source bucket — check the address, bucket and keys.".into()) })?;

    let id = crate::Id::new();
    sqlx::query("INSERT INTO s3_import_jobs (id, source, system_id, started_by) VALUES ($1, $2, $3, $4)")
        .bind(id).bind(format!("{}/{}/{}", src.endpoint.trim_end_matches('/'), src.bucket, src.prefix))
        .bind(&src.system_id).bind(&admin.0.id).execute(&s.db).await?;
    crate::basket::log_activity(&s.db, &admin.0, "started import from S3", &format!("{}/{}", src.bucket, src.prefix), "import", None, &src.system_id).await;
    let (s2, who) = (s.clone(), admin.0.clone());
    tokio::spawn(async move {
        let result = run(&s2, &who, &store, &src.prefix, &src.system_id, &bucket, id).await;
        let (status, err) = match result { Ok(()) => ("done", None), Err(e) => ("failed", Some(e.to_string())) };
        let _ = sqlx::query("UPDATE s3_import_jobs SET status = $2, last_error = COALESCE($3, last_error), finished_at = now() WHERE id = $1")
            .bind(id).bind(status).bind(err).execute(&s2.db).await;
    });
    Ok(Json(json!({ "id": id })))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Job {
    id: Id, source: String, system_id: String, status: String, copied: i32, skipped: i32, failed: i32, bytes: i64,
    last_error: Option<String>, started_at: chrono::DateTime<chrono::Utc>, finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// GET /api/admin/import-s3/:id — progress.
pub async fn status(State(s): State<Arc<AppState>>, _: AdminUser, Path(id): Path<Id>) -> ApiResult<Json<Job>> {
    if !crate::s3::enabled() { return Err(ApiError::NotFound); }
    Ok(Json(sqlx::query_as("SELECT id, source, system_id, status, copied, skipped, failed, bytes, last_error, started_at, finished_at FROM s3_import_jobs WHERE id = $1")
        .bind(id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?))
}

async fn run(s: &Arc<AppState>, user: &User, store: &S3Store, prefix: &str, sys: &str, bucket: &str, job: Id) -> anyhow::Result<()> {
    let mut token: Option<String> = None;
    loop {
        let (items, next) = store.list_page(prefix, token.as_deref()).await?;
        // A few objects at a time: fast, without flooding the database.
        futures::stream::iter(items).for_each_concurrent(4, |(key, size)| async move {
            let outcome = copy_one(s, user, store, sys, bucket, &key, size).await;
            let (col, err) = match &outcome { Ok(true) => ("copied", None), Ok(false) => ("skipped", None), Err(e) => ("failed", Some(format!("{key}: {e}"))) };
            let added = if matches!(outcome, Ok(true)) { size as i64 } else { 0 };
            let _ = sqlx::query(&format!("UPDATE s3_import_jobs SET {col} = {col} + 1, bytes = bytes + $2, last_error = COALESCE($3, last_error) WHERE id = $1"))
                .bind(job).bind(added).bind(err).execute(&s.db).await;
        }).await;
        match next { Some(t) => token = Some(t), None => return Ok(()) }
    }
}

/// Ok(true) = copied, Ok(false) = already there with the same size.
async fn copy_one(s: &Arc<AppState>, user: &User, store: &S3Store, sys: &str, _bucket: &str, key: &str, size: u64) -> anyhow::Result<bool> {
    if key.ends_with('/') { return Ok(false); }
    if let Ok(existing) = crate::s3::find_object(s, user, sys, key).await {
        if existing.size_bytes as u64 == size { return Ok(false); }
    }
    let mut body = store.get_object(key).await?.bytes_stream();
    let dir = crate::storage::spool_root().join(".s3");
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("import-{}", crate::Id::new()));
    let mut out = tokio::fs::File::create(&path).await?;
    while let Some(chunk) = body.next().await { out.write_all(&chunk?).await?; }
    out.flush().await?;
    crate::s3::put_object_from(s, user, sys, key, path, &axum::http::HeaderMap::new()).await
        .map_err(|e| anyhow::anyhow!("{}", e.msg))?;
    Ok(true)
}

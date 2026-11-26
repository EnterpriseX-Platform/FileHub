//! Rate limits shared by every FileHub pod.
//!
//! One fixed one-minute window per key, kept in the UNLOGGED `rate_hits`
//! table: a single upsert per call, no process-wide lock, and the same count
//! whichever pod a request lands on. Keys are namespaced by the caller
//! (`upload:<user>`, `login:<email>`, `share-miss:<ip>`, …).

use sqlx::PgPool;

use crate::error::ApiError;

/// Count one hit for `key`; refuse once it goes past `limit` this minute.
pub async fn hit(db: &PgPool, key: &str, limit: u32, message: &str) -> Result<(), ApiError> {
    let hits: i32 = sqlx::query_scalar(
        "INSERT INTO rate_hits (key, window_start, hits) VALUES ($1, date_trunc('minute', now()), 1)
         ON CONFLICT (key) DO UPDATE
            SET hits = CASE WHEN rate_hits.window_start = EXCLUDED.window_start THEN rate_hits.hits + 1 ELSE 1 END,
                window_start = EXCLUDED.window_start
         RETURNING hits",
    ).bind(key).fetch_one(db).await?;
    if hits as u32 > limit { return Err(ApiError::TooManyRequests(message.into())); }
    Ok(())
}

/// Refuse if `key` is already at `limit` this minute (doesn't count a hit).
pub async fn check(db: &PgPool, key: &str, limit: u32, message: &str) -> Result<(), ApiError> {
    let hits: Option<i32> = sqlx::query_scalar("SELECT hits FROM rate_hits WHERE key = $1 AND window_start = date_trunc('minute', now())")
        .bind(key).fetch_optional(db).await?;
    if hits.unwrap_or(0) as u32 >= limit { return Err(ApiError::TooManyRequests(message.into())); }
    Ok(())
}

/// Forget `key` (e.g. after a successful sign-in).
pub async fn reset(db: &PgPool, key: &str) {
    let _ = sqlx::query("DELETE FROM rate_hits WHERE key = $1").bind(key).execute(db).await;
}

/// Drop windows that ended (called by the background worker).
pub async fn sweep(db: &PgPool) -> Result<u64, ApiError> {
    let _ = sqlx::query("DELETE FROM connector_calls WHERE at < now() - interval '180 days'").execute(db).await;
    Ok(sqlx::query("DELETE FROM rate_hits WHERE window_start < date_trunc('minute', now())").execute(db).await?.rows_affected())
}

/// Uploads per person per minute (`UPLOAD_RATE_PER_MIN`, default 60).
pub fn uploads_per_minute() -> u32 {
    static N: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *N.get_or_init(|| crate::config::env_num("UPLOAD_RATE_PER_MIN", 60) as u32)
}

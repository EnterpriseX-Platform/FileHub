//! Webhooks: tell other systems when files arrive or change (like MinIO bucket
//! notifications), without them polling.
//!
//! Events are written to an outbox (`webhook_deliveries`) in the same request,
//! one statement, so a slow or dead receiver never slows an upload. A worker
//! sends them: POST JSON, signed with HMAC-SHA256 in `X-FileHub-Signature`,
//! retried with backoff (1, 2, 4 … 60 min) up to 10 times. `SKIP LOCKED` lets
//! every pod run the worker without sending anything twice.
//!
//! Events: `file.uploaded`, `file.version`, `file.deleted`, `file.restored`,
//! `file.status`.

use std::sync::Arc;
use std::time::Duration;

use axum::{extract::{Path, State}, http::StatusCode, Json};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use sqlx::PgPool;
use crate::Id;

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const EVENTS: [&str; 5] = ["file.uploaded", "file.version", "file.deleted", "file.restored", "file.status"];
const MAX_ATTEMPTS: i32 = 10;

/// Events some active webhook subscribes to, cached per pod for 10 s and
/// cleared when webhooks change, so an upload pays nothing when none listen.
static WANTED: std::sync::Mutex<Option<(std::time::Instant, Vec<String>)>> = std::sync::Mutex::new(None);

async fn wanted(db: &PgPool, event: &str) -> bool {
    if let Some((at, events)) = WANTED.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(10) { return events.iter().any(|e| e == event); }
    }
    let events: Vec<String> = sqlx::query_scalar("SELECT DISTINCT unnest(events) FROM webhooks WHERE active")
        .fetch_all(db).await.unwrap_or_default();
    let hit = events.iter().any(|e| e == event);
    *WANTED.lock().unwrap() = Some((std::time::Instant::now(), events));
    hit
}

/// Reload the subscriptions on the next event (other pods within 10 s).
fn forget_wanted() { *WANTED.lock().unwrap() = None; }

/// Queue `event` for every active webhook that wants it (all baskets, or this one).
pub async fn emit(db: &PgPool, event: &str, system_id: &str, data: Value) {
    if !wanted(db, event).await { return; }
    let payload = json!({ "event": event, "basket": system_id, "at": chrono::Utc::now(), "data": data });
    let r = sqlx::query(
        "INSERT INTO webhook_deliveries (id, webhook_id, event, payload)
         SELECT fh_cuid(), id, $1, $3 FROM webhooks
          WHERE active AND $1 = ANY(events) AND (system_id IS NULL OR system_id = $2)",
    ).bind(event).bind(system_id).bind(&payload).execute(db).await;
    if let Err(e) = r { tracing::warn!("webhook outbox: {e}"); }
}

/// The file fields a receiver needs (no storage keys or internal paths).
pub fn file_data(f: &crate::models::File) -> Value {
    json!({ "id": f.id, "name": f.name, "size": f.size_bytes, "version": f.version, "status": f.status,
            "folder_id": f.folder_id, "created_by": f.created_by })
}

// ── Delivery worker ──

/// Runs forever: sends due deliveries every couple of seconds.
pub async fn run_worker(db: PgPool) {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::none()).build().expect("http client");
    loop {
        match deliver_due(&db, &client).await {
            Ok(n) if n > 0 => continue, // more may be waiting
            Ok(_) => {}
            Err(e) => tracing::warn!("webhook worker: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn deliver_due(db: &PgPool, client: &reqwest::Client) -> Result<usize, sqlx::Error> {
    let mut tx = db.begin().await?;
    let due: Vec<(Id, String, Value, i32, String, String)> = sqlx::query_as(
        "SELECT d.id, d.event, d.payload, d.attempts, w.url, w.secret
           FROM webhook_deliveries d JOIN webhooks w ON w.id = d.webhook_id
          WHERE d.delivered_at IS NULL AND d.next_at <= now() AND w.active
          ORDER BY d.next_at LIMIT 50
          FOR UPDATE OF d SKIP LOCKED",
    ).fetch_all(&mut *tx).await?;
    let n = due.len();
    for (id, event, payload, attempts, url, secret) in due {
        let (status, error) = send(client, &url, &secret, &event, &payload).await;
        if status.map(|c| (200..300).contains(&c)).unwrap_or(false) {
            sqlx::query("UPDATE webhook_deliveries SET delivered_at = now(), attempts = attempts + 1, last_status = $2, last_error = NULL WHERE id = $1")
                .bind(id).bind(status.map(i32::from)).execute(&mut *tx).await?;
        } else {
            let tries = attempts + 1;
            // 1, 2, 4 … minutes, at most an hour; after the last try it stays failed.
            let next = if tries >= MAX_ATTEMPTS { "'infinity'::timestamptz".to_string() } else { format!("now() + interval '{} minutes'", (1i64 << (tries - 1).min(6)).min(60)) };
            sqlx::query(&format!("UPDATE webhook_deliveries SET attempts = $2, last_status = $3, last_error = $4, next_at = {next} WHERE id = $1"))
                .bind(id).bind(tries).bind(status.map(i32::from)).bind(error).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(n)
}

async fn send(client: &reqwest::Client, url: &str, secret: &str, event: &str, payload: &Value) -> (Option<u16>, Option<String>) {
    match reqwest::Url::parse(url) {
        Ok(u) if outbound_allowed(&u).await => {}
        _ => return (None, Some("address refused".into())),
    }
    let body = payload.to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(body.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    match client.post(url).header("content-type", "application/json").header("x-filehub-event", event)
        .header("x-filehub-signature", format!("sha256={sig}")).body(body).send().await {
        Ok(r) => (Some(r.status().as_u16()), (!r.status().is_success()).then(|| format!("HTTP {}", r.status()))),
        Err(e) => (None, Some(e.to_string())),
    }
}

// ── Admin REST ──

#[derive(Serialize, sqlx::FromRow)]
pub struct Hook { id: Id, name: String, url: String, system_id: Option<String>, events: Vec<String>, active: bool, created_at: chrono::DateTime<chrono::Utc> }

#[derive(Deserialize)]
pub struct NewHook { name: String, url: String, system_id: Option<String>, events: Vec<String> }

async fn check_url(url: &str) -> Result<(), ApiError> {
    let u = reqwest::Url::parse(url).map_err(|_| ApiError::BadRequest("Enter a full http(s) address.".into()))?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() { return Err(ApiError::BadRequest("Enter a full http(s) address.".into())); }
    if !outbound_allowed(&u).await { return Err(ApiError::BadRequest("FileHub doesn't call that address.".into())); }
    Ok(())
}

/// Server-side request forgery guard for addresses an admin types in (webhooks,
/// S3 import). Loopback, link-local (cloud metadata) and unspecified addresses
/// are always refused; private networks too when OUTBOUND_BLOCK_PRIVATE=1.
/// Checked on every call, so a DNS name that later points inside is caught.
pub async fn outbound_allowed(u: &reqwest::Url) -> bool {
    use std::net::IpAddr;
    let Some(host) = u.host_str() else { return false };
    let port = u.port_or_known_default().unwrap_or(80);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let Ok(addrs) = tokio::net::lookup_host((host, port)).await else { return false };
    let block_private = crate::config::env_flag("OUTBOUND_BLOCK_PRIVATE");
    // Local development only: a receiver on this machine.
    let allow_loopback = crate::config::env_flag("OUTBOUND_ALLOW_LOOPBACK");
    let mut any = false;
    for a in addrs {
        any = true;
        let bad = match a.ip() {
            IpAddr::V4(v) => (v.is_loopback() && !allow_loopback) || v.is_link_local() || v.is_unspecified() || v.is_broadcast()
                || (block_private && v.is_private()),
            IpAddr::V6(v) => (v.is_loopback() && !allow_loopback) || v.is_unspecified() || (v.segments()[0] & 0xffc0) == 0xfe80
                || v.to_ipv4_mapped().is_some_and(|m| m.is_loopback() || m.is_link_local())
                || (block_private && (v.segments()[0] & 0xfe00) == 0xfc00),
        };
        if bad { return false; }
    }
    any
}

pub async fn list(State(s): State<Arc<AppState>>, _: AdminUser) -> ApiResult<Json<Vec<Hook>>> {
    Ok(Json(sqlx::query_as("SELECT id, name, url, system_id, events, active, created_at FROM webhooks ORDER BY created_at DESC").fetch_all(&s.db).await?))
}

pub async fn create(State(s): State<Arc<AppState>>, admin: AdminUser, Json(b): Json<NewHook>) -> ApiResult<Json<Value>> {
    check_url(&b.url).await?;
    if b.name.trim().is_empty() { return Err(ApiError::BadRequest("Give the webhook a name.".into())); }
    if b.events.is_empty() || b.events.iter().any(|e| !EVENTS.contains(&e.as_str())) {
        return Err(ApiError::BadRequest(format!("Pick events from: {}.", EVENTS.join(", "))));
    }
    let id = crate::Id::new();
    let secret = hex::encode(rand::random::<[u8; 24]>());
    sqlx::query("INSERT INTO webhooks (id, name, url, secret, system_id, events, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(b.name.trim()).bind(&b.url).bind(&secret).bind(b.system_id.filter(|v| !v.is_empty())).bind(&b.events).bind(&admin.0.id)
        .execute(&s.db).await?;
    forget_wanted();
    crate::basket::log_activity(&s.db, &admin.0, "created webhook", b.name.trim(), "webhook", None, "").await;
    // The signing secret is shown once.
    Ok(Json(json!({ "id": id, "secret": secret })))
}

pub async fn remove(State(s): State<Arc<AppState>>, admin: AdminUser, Path(id): Path<Id>) -> ApiResult<StatusCode> {
    let name: Option<String> = sqlx::query_scalar("DELETE FROM webhooks WHERE id = $1 RETURNING name").bind(id).fetch_optional(&s.db).await?;
    forget_wanted();
    if let Some(n) = name { crate::basket::log_activity(&s.db, &admin.0, "deleted webhook", &n, "webhook", None, "").await; }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Delivery { id: Id, event: String, attempts: i32, last_status: Option<i32>, last_error: Option<String>, delivered_at: Option<chrono::DateTime<chrono::Utc>>, created_at: chrono::DateTime<chrono::Utc> }

pub async fn deliveries(State(s): State<Arc<AppState>>, _: AdminUser, Path(id): Path<Id>) -> ApiResult<Json<Vec<Delivery>>> {
    Ok(Json(sqlx::query_as("SELECT id, event, attempts, last_status, last_error, delivered_at, created_at FROM webhook_deliveries WHERE webhook_id = $1 ORDER BY created_at DESC LIMIT 50")
        .bind(id).fetch_all(&s.db).await?))
}

/// Send a `ping` event now (queued like any other).
pub async fn test(State(s): State<Arc<AppState>>, _: AdminUser, Path(id): Path<Id>) -> ApiResult<StatusCode> {
    sqlx::query("INSERT INTO webhook_deliveries (id, webhook_id, event, payload) VALUES ($1, $2, 'ping', $3)")
        .bind(crate::Id::new()).bind(id).bind(json!({ "event": "ping", "at": chrono::Utc::now() })).execute(&s.db).await?;
    Ok(StatusCode::ACCEPTED)
}

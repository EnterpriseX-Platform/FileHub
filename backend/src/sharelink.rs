//! Public links: who can take a file without signing in, and the guards around it.
//!
//! * token: 24 random bytes (192 bits) — not guessable; failed lookups and wrong
//!   passwords are throttled per client address
//! * always expire (1–90 days), can be turned off, can need a password, can stop
//!   after N downloads; the file going to trash or losing its basket's permission
//!   for public links kills the link
//! * the public page sees only name, size, type and expiry — never storage keys,
//!   owner, fields or ids
//! * every public download is counted and logged (with the client address);
//!   baskets that stamp downloads stamp public ones too ("Public link <id>")

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use crate::Id;

use crate::auth::{AuthUser, MaybeAuthUser};
use crate::error::{ApiError, ApiResult};
use crate::handlers::FILE_COLS;
use crate::models::File;
use crate::state::AppState;

fn bad(m: impl Into<String>) -> ApiError { ApiError::BadRequest(m.into()) }

/// Client address as our own proxy saw it: X-Real-IP, else the right-most
/// X-Forwarded-For entry (the one our ingress appended). The left-most entries
/// are whatever the client claimed and can't be trusted for throttling.
pub fn client_addr(h: &HeaderMap) -> String {
    h.get("x-real-ip").and_then(|v| v.to_str().ok()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
        .or_else(|| h.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit(',').next()).map(|v| v.trim().to_string()))
        .unwrap_or_else(|| "direct".into())
}

/// Per-address throttle for misses (unknown token, wrong password): 20/min.
async fn miss(db: &sqlx::PgPool, h: &HeaderMap) -> ApiError {
    if let Err(e) = crate::auth::upload_rate_limit_keyed(db, &format!("share-miss:{}", client_addr(h)), 20).await { return e; }
    ApiError::NotFound
}
async fn guard(db: &sqlx::PgPool, h: &HeaderMap) -> Result<(), ApiError> {
    // Refuse early once an address has spent its misses (checked without charging).
    crate::auth::rate_peek(db, &format!("share-miss:{}", client_addr(h)), 20).await
}

#[derive(Debug, Deserialize)]
pub struct NewLink {
    pub expires_in_days: Option<i64>,
    pub password: Option<String>,
    pub max_downloads: Option<i32>,
    pub note: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    /// "view" (look only) or "download" (default).
    #[serde(default)]
    pub access: Option<String>,
    /// true = only people signed in to FileHub can use the link.
    #[serde(default)]
    pub require_login: Option<bool>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct LinkRow {
    id: Id,
    token: String,
    expires_at: Option<DateTime<Utc>>,
    created_by: String,
    note: Option<String>,
    created_at: DateTime<Utc>,
    max_downloads: Option<i32>,
    download_count: i32,
    has_password: bool,
    last_access_at: Option<DateTime<Utc>>,
    access: String,
    require_login: bool,
}

pub async fn create(State(s): State<Arc<AppState>>, user: AuthUser, Path(file_id): Path<Id>, Json(c): Json<NewLink>) -> ApiResult<Json<Value>> {
    let ext = sqlx::query_scalar::<_, String>("SELECT value FROM workspace_config WHERE key = 'allow_external_sharing'").fetch_optional(&s.db).await?;
    if matches!(ext.as_deref(), Some("false") | Some("0") | Some("off")) { return Err(ApiError::Forbidden); }
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(file_id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file.id, crate::acl::EDIT).await?;
    let pol = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?;
    if pol.public_links == Some(false) || crate::basket::is_vault(&s.db, &file.system_id).await? {
        return Err(bad("Public links are turned off for this basket."));
    }
    let days = c.expires_in_days.unwrap_or(7).clamp(1, 90);
    if let Some(m) = c.max_downloads { if !(1..=100_000).contains(&m) { return Err(bad("Most downloads must be 1–100,000.")); } }
    let hash = match c.password.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) if p.len() < 6 => return Err(bad("Use a password of at least 6 characters.")),
        Some(p) => Some(crate::auth::hash_password(p)?),
        None => None,
    };
    let access = match c.access.as_deref().unwrap_or("download") {
        a @ ("view" | "download") => a.to_string(),
        _ => return Err(bad("access must be view or download")),
    };
    let require_login = c.require_login.unwrap_or(false);
    let token = crate::handlers::random_token();
    let id = crate::Id::new();
    let expires = Utc::now() + Duration::days(days);
    sqlx::query("INSERT INTO share_links (id, file_id, token, expires_at, created_by, note, created_by_id, password_hash, max_downloads, access, require_login) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(id).bind(file.id).bind(&token).bind(expires).bind(&user.0.display_name).bind(&c.note).bind(&user.0.id)
        .bind(&hash).bind(c.max_downloads).bind(&access).bind(require_login).execute(&s.db).await?;
    crate::basket::log_activity(&s.db, &user.0, &format!("created public link (expires in {days} days{})", if hash.is_some() { ", password" } else { "" }), &file.name, "file", Some(file.id), &file.system_id).await;
    Ok(Json(json!({ "id": id, "token": token, "url": format!("/filehub/s/{token}"), "expires_at": expires, "has_password": hash.is_some(), "max_downloads": c.max_downloads, "access": access, "require_login": require_login })))
}

pub async fn list(State(s): State<Arc<AppState>>, user: AuthUser, Path(file_id): Path<Id>) -> ApiResult<Json<Vec<LinkRow>>> {
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::EDIT).await?;
    Ok(Json(sqlx::query_as(
        "SELECT id, token, expires_at, created_by, note, created_at, max_downloads, download_count, password_hash IS NOT NULL AS has_password, last_access_at, access, require_login
           FROM share_links WHERE file_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now()) ORDER BY created_at DESC",
    ).bind(file_id).fetch_all(&s.db).await?))
}

pub async fn revoke(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<StatusCode> {
    let row: Option<(Id,)> = sqlx::query_as("SELECT file_id FROM share_links WHERE id = $1 AND revoked_at IS NULL").bind(id).fetch_optional(&s.db).await?;
    let (fid,) = row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, fid, crate::acl::EDIT).await?;
    sqlx::query("UPDATE share_links SET revoked_at = now() WHERE id = $1").bind(id).execute(&s.db).await?;
    let (sys, name): (String, String) = sqlx::query_as("SELECT system_id, name FROM files WHERE id = $1").bind(fid).fetch_one(&s.db).await?;
    crate::basket::log_activity(&s.db, &user.0, "turned off a public link", &name, "file", Some(fid), &sys).await;
    Ok(StatusCode::NO_CONTENT)
}

// -----------------------------------------------------------------------------
// Public side
// -----------------------------------------------------------------------------

struct Live { id: Id, file: File, expires_at: Option<DateTime<Utc>>, password_hash: Option<String>, left: Option<i32>, access: String, require_login: bool }

async fn live(s: &AppState, token: &str, h: &HeaderMap) -> Result<Live, ApiError> {
    guard(&s.db, h).await?;
    if token.len() < 20 || token.len() > 64 { return Err(miss(&s.db, h).await); }
    #[allow(clippy::type_complexity)]
    let row: Option<(Id, Id, Option<DateTime<Utc>>, Option<String>, Option<i32>, i32, Option<DateTime<Utc>>, String, bool)> = sqlx::query_as(
        "SELECT id, file_id, expires_at, password_hash, max_downloads, download_count, revoked_at, access, require_login FROM share_links WHERE token = $1",
    ).bind(token).fetch_optional(&s.db).await?;
    let Some((id, fid, exp, ph, max, count, revoked, access, require_login)) = row else { return Err(miss(&s.db, h).await) };
    if revoked.is_some() || exp.map(|e| Utc::now() > e).unwrap_or(false) { return Err(ApiError::NotFound); }
    let left = max.map(|m| (m - count).max(0));
    if left == Some(0) { return Err(ApiError::NotFound); }
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(fid).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    // The basket may have turned public links off (or become a Vault) after the link was made.
    let pol = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?;
    if pol.public_links == Some(false) || crate::basket::is_vault(&s.db, &file.system_id).await? { return Err(ApiError::NotFound); }
    Ok(Live { id, file, expires_at: exp, password_hash: ph, left, access, require_login })
}

/// What the public page may know: nothing that identifies storage or people.
pub async fn meta(State(s): State<Arc<AppState>>, user: MaybeAuthUser, headers: HeaderMap, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let l = live(&s, &token, &headers).await?;
    let signed_in = user.0.is_some();
    // A sign-in-only link tells an anonymous visitor nothing but that.
    if l.require_login && !signed_in {
        return Ok(Json(json!({ "login_required": true, "signed_in": false })));
    }
    Ok(Json(json!({
        "name": l.file.name, "size_bytes": l.file.size_bytes, "file_type": l.file.file_type,
        "modified_at": l.file.modified_at, "expires_at": l.expires_at,
        "password_required": l.password_hash.is_some(), "downloads_left": l.left,
        "access": l.access, "login_required": l.require_login, "signed_in": signed_in,
        "viewable": viewable(&l.file.file_type),
    })))
}

/// Types the public page can show in the browser.
fn viewable(t: &str) -> bool {
    matches!(t, "pdf" | "png" | "jpg" | "jpeg" | "gif" | "img" | "webp" | "txt" | "csv" | "mp4" | "webm" | "mp3" | "wav"
        | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx")
}

fn need_login() -> ApiError {
    ApiError::Coded(StatusCode::UNAUTHORIZED, "login_required", "ลิงก์นี้ใช้ได้เฉพาะผู้ที่เข้าสู่ระบบแล้ว กรุณาเข้าสู่ระบบก่อน".into())
}

#[derive(Debug, Deserialize, Default)]
pub struct Unlock { pub password: Option<String> }

/// GET for links without a password; POST {password} otherwise (never in the URL).
pub async fn download_get(State(s): State<Arc<AppState>>, user: MaybeAuthUser, headers: HeaderMap, Path(token): Path<String>) -> ApiResult<axum::response::Response> {
    serve(&s, &headers, &token, None, user.0.is_some()).await
}
pub async fn download_post(State(s): State<Arc<AppState>>, user: MaybeAuthUser, headers: HeaderMap, Path(token): Path<String>, Json(b): Json<Unlock>) -> ApiResult<axum::response::Response> {
    serve(&s, &headers, &token, b.password, user.0.is_some()).await
}
pub async fn view_get(State(s): State<Arc<AppState>>, user: MaybeAuthUser, headers: HeaderMap, Path(token): Path<String>) -> ApiResult<axum::response::Response> {
    view(&s, &headers, &token, None, user.0.is_some()).await
}
pub async fn view_post(State(s): State<Arc<AppState>>, user: MaybeAuthUser, headers: HeaderMap, Path(token): Path<String>, Json(b): Json<Unlock>) -> ApiResult<axum::response::Response> {
    view(&s, &headers, &token, b.password, user.0.is_some()).await
}

/// The link's checks that come before any bytes: signed in when required, then the password.
async fn unlock(s: &Arc<AppState>, h: &HeaderMap, l: &Live, password: Option<String>, signed_in: bool) -> ApiResult<()> {
    if l.require_login && !signed_in { return Err(need_login()); }
    if let Some(hash) = &l.password_hash {
        crate::ratelimit::hit(&s.db, &format!("share-pw:{}", l.id), 10, "ใส่รหัสผ่านผิดหลายครั้งเกินไป กรุณารอ 1 นาทีแล้วลองใหม่").await?;
        let given = password.filter(|p| !p.is_empty());
        let Some(p) = given else {
            return Err(ApiError::Coded(StatusCode::UNAUTHORIZED, "password_required", "ลิงก์นี้ต้องใส่รหัสผ่านก่อน กรุณาใส่รหัสผ่าน".into()));
        };
        let hash = hash.clone();
        let ok = tokio::task::spawn_blocking(move || crate::auth::verify_password(&p, &hash)).await.unwrap_or(false);
        if !ok {
            let _ = miss(&s.db, h).await;
            return Err(ApiError::Coded(StatusCode::UNAUTHORIZED, "wrong_password", "รหัสผ่านไม่ถูกต้อง".into()));
        }
    }
    Ok(())
}

/// Show the file in the browser (view-only and download links both may).
async fn view(s: &Arc<AppState>, h: &HeaderMap, token: &str, password: Option<String>, signed_in: bool) -> ApiResult<axum::response::Response> {
    let l = live(s, token, h).await?;
    unlock(s, h, &l, password, signed_in).await?;
    if !viewable(&l.file.file_type) {
        return Err(ApiError::Coded(StatusCode::UNSUPPORTED_MEDIA_TYPE, "not_viewable", "ไฟล์ประเภทนี้เปิดดูในเบราว์เซอร์ไม่ได้".into()));
    }
    let addr = client_addr(h);
    let _ = sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, created_at)
           VALUES ($1,$2,'slate','viewed through a public link',$3,'file',$4,$5,now())"#,
    ).bind(crate::Id::new()).bind(format!("Public link ({addr})")).bind(&l.file.name).bind(l.file.id).bind(&l.file.system_id).execute(&s.db).await;
    let pol = crate::basket::effective_policy(&s.db, &l.file.system_id, l.file.folder_id.as_deref()).await?;
    let office = matches!(l.file.file_type.as_str(), "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx");
    if (pol.stamp_downloads == Some(true) && matches!(l.file.file_type.as_str(), "pdf" | "docx" | "xlsx" | "pptx")) || office {
        if pol.stamp_downloads == Some(true) {
            let who = public_user(&l, &addr);
            return crate::handlers::stamped_download(s, &who, &l.file, true).await;
        }
        // Office: the PDF preview made at upload (or now).
        let cached: Option<Vec<u8>> = sqlx::query_scalar("SELECT data FROM file_previews WHERE file_id = $1 AND mime = 'application/pdf'")
            .bind(l.file.id).fetch_optional(&s.db).await?;
        let pdf = match cached {
            Some(p) => p,
            None => {
                crate::jobs::fits_in_memory(l.file.size_bytes, "preview")?;
                let (body, _) = s.storage.get(&l.file.object_key, l.file.encrypted).await?.ok_or(ApiError::NotFound)?;
                crate::jobs::office_to_pdf(l.file.name.clone(), body.to_vec()).await
                    .ok_or_else(|| ApiError::Coded(StatusCode::UNSUPPORTED_MEDIA_TYPE, "not_viewable", "แปลงไฟล์นี้เพื่อเปิดดูไม่สำเร็จ".into()))?
            }
        };
        let mut hm = HeaderMap::new();
        hm.insert(axum::http::header::CONTENT_TYPE, "application/pdf".parse().unwrap());
        hm.insert(axum::http::header::CONTENT_DISPOSITION, "inline".parse().unwrap());
        hm.insert(axum::http::header::CACHE_CONTROL, "private, no-store".parse().unwrap());
        use axum::response::IntoResponse;
        return Ok((StatusCode::OK, hm, pdf).into_response());
    }
    crate::serve::stream_object(s, crate::serve::ServeOpts {
        key: &l.file.object_key, encrypted: l.file.encrypted, name: &l.file.name, etag: l.file.etag.as_deref(),
        disposition: "inline", cache_control: "private, no-store",
    }, h).await
}

fn public_user(l: &Live, addr: &str) -> crate::auth::User {
    crate::auth::User { id: "public".into(), email: format!("public-link-{}", &l.id.as_str()[..8]),
        display_name: format!("Public link ({addr})"), avatar_tone: "slate".into(), password_hash: String::new(),
        role: "viewer".into(), status: "active".into(), created_at: Utc::now() }
}

async fn serve(s: &Arc<AppState>, h: &HeaderMap, token: &str, password: Option<String>, signed_in: bool) -> ApiResult<axum::response::Response> {
    let l = live(s, token, h).await?;
    unlock(s, h, &l, password, signed_in).await?;
    if l.access == "view" {
        return Err(ApiError::Coded(StatusCode::FORBIDDEN, "view_only", "ลิงก์นี้ให้เปิดดูได้อย่างเดียว ดาวน์โหลดไม่ได้".into()));
    }
    // Count first, atomically, so a limit can't be raced past.
    let n = sqlx::query("UPDATE share_links SET download_count = download_count + 1, last_access_at = now()
                          WHERE id = $1 AND (max_downloads IS NULL OR download_count < max_downloads)")
        .bind(l.id).execute(&s.db).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound); }
    let addr = client_addr(h);
    let _ = sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, created_at)
           VALUES ($1,$2,'slate','downloaded through a public link',$3,'file',$4,$5,now())"#,
    ).bind(crate::Id::new()).bind(format!("Public link ({addr})")).bind(&l.file.name).bind(l.file.id).bind(&l.file.system_id).execute(&s.db).await;

    let pol = crate::basket::effective_policy(&s.db, &l.file.system_id, l.file.folder_id.as_deref()).await?;
    if pol.stamp_downloads == Some(true) && matches!(l.file.file_type.as_str(), "pdf" | "docx" | "xlsx" | "pptx") {
        let who = public_user(&l, &addr);
        return crate::handlers::stamped_download(s, &who, &l.file, false).await;
    }
    crate::serve::stream_object(s, crate::serve::ServeOpts {
        key: &l.file.object_key, encrypted: l.file.encrypted, name: &l.file.name, etag: l.file.etag.as_deref(),
        disposition: "attachment", cache_control: "private, no-store",
    }, h).await
}

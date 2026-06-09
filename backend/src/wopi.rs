//! WOPI host — the protocol Microsoft Office Online, OnlyOffice, and
//! Collabora Online all speak to fetch + save documents from a storage
//! backend.  The full spec is enormous but only three endpoints are
//! required for view/edit:
//!
//! ```text
//! GET  /wopi/files/:id                  CheckFileInfo  (metadata JSON)
//! GET  /wopi/files/:id/contents         GetFile        (raw bytes)
//! POST /wopi/files/:id/contents         PutFile        (save edits)
//! ```
//!
//! Every request includes `access_token=<token>` in the query string.  The
//! token is a short-lived HMAC-SHA256 we issue when the browser opens a
//! file; it embeds the file id + caller user id + permission + expiry so
//! we don't have to look anything up except the HMAC.
//!
//! Vendor-neutral by design: switching from Collabora to OnlyOffice or to
//! a future home-grown viewer only changes the iframe URL we hand to the
//! browser — these endpoints stay identical.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

/// 1 hour is long enough to read or finish a small edit but short enough
/// that a stolen URL doesn't grant indefinite access.
const TOKEN_TTL_SECS: i64 = 3600;

pub const WOPI_SECRET_ENV: &str = "WOPI_SECRET";
/// Fallback used only in debug builds. Release builds refuse to boot without
/// `WOPI_SECRET` set — see `AppState::init` for the fail-fast check.
const WOPI_SECRET_DEV_FALLBACK: &str = "dev-only-wopi-secret-change-me";

fn secret() -> Vec<u8> {
    // In release the boot check guarantees this env is set; in debug we keep
    // a fallback so `cargo test`/local checkouts don't need bespoke setup.
    // If a release deploy somehow lost the env between boot and request, we
    // log loudly and refuse to mint a forged-able token (panic surfaces as
    // 500, which is the right answer — better than handing out a token
    // signed with a public key).
    match std::env::var(WOPI_SECRET_ENV) {
        Ok(v) if !v.is_empty() => v.into_bytes(),
        _ => {
            #[cfg(debug_assertions)] { return WOPI_SECRET_DEV_FALLBACK.as_bytes().to_vec(); }
            #[cfg(not(debug_assertions))] {
                tracing::error!("WOPI_SECRET unset at request time — refusing to sign token");
                panic!("WOPI_SECRET must be set in release builds");
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenClaims {
    file_id: Uuid,
    user_id: String,
    /// "view" or "edit".  Save handler refuses if not "edit".
    perm: String,
    /// Unix seconds.
    exp: i64,
}

/// Sign claims into an opaque URL-safe token: `base64(claims) + '.' + base64(hmac)`.
pub fn sign_token(file_id: Uuid, user_id: &str, perm: &str) -> ApiResult<String> {
    let claims = TokenClaims {
        file_id,
        user_id: user_id.to_string(),
        perm: perm.to_string(),
        exp: Utc::now().timestamp() + TOKEN_TTL_SECS,
    };
    let payload = serde_json::to_vec(&claims)
        .map_err(|e| ApiError::Other(anyhow::anyhow!("token encode: {e}")))?;
    let b64 = URL_SAFE_NO_PAD.encode(&payload);
    let mut mac = HmacSha256::new_from_slice(&secret())
        .map_err(|e| ApiError::Other(anyhow::anyhow!("hmac key: {e}")))?;
    mac.update(b64.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(format!("{b64}.{sig}"))
}

fn verify_token(token: &str) -> Result<TokenClaims, ApiError> {
    let (b64, sig) = token.split_once('.').ok_or(ApiError::Unauthorized)?;
    let mut mac = HmacSha256::new_from_slice(&secret())
        .map_err(|_| ApiError::Unauthorized)?;
    mac.update(b64.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    // Constant-time compare guards against timing-based token forgery.
    if !constant_time_eq(expected.as_bytes(), sig.as_bytes()) {
        return Err(ApiError::Unauthorized);
    }
    let payload = URL_SAFE_NO_PAD.decode(b64).map_err(|_| ApiError::Unauthorized)?;
    let claims: TokenClaims = serde_json::from_slice(&payload).map_err(|_| ApiError::Unauthorized)?;
    if claims.exp < Utc::now().timestamp() {
        return Err(ApiError::Unauthorized);
    }
    Ok(claims)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}

// -----------------------------------------------------------------------------
// CheckFileInfo — GET /wopi/files/:id?access_token=…
// -----------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct TokenQuery {
    pub access_token: String,
}

#[derive(Debug, Serialize)]
pub struct CheckFileInfo {
    // Required keys per WOPI spec.
    #[serde(rename = "BaseFileName")]      base_file_name: String,
    #[serde(rename = "Size")]              size: i64,
    #[serde(rename = "Version")]           version: String,
    #[serde(rename = "OwnerId")]           owner_id: String,
    #[serde(rename = "UserId")]            user_id: String,
    #[serde(rename = "UserFriendlyName")]  user_friendly_name: String,

    // Permissions — toggled by the access-token's `perm` claim so an admin
    // can issue a view-only link and an editor can issue a write link from
    // the same backend.
    #[serde(rename = "UserCanWrite")]      user_can_write: bool,
    #[serde(rename = "ReadOnly")]          read_only: bool,
    #[serde(rename = "DisableExport")]     disable_export: bool,
    #[serde(rename = "DisablePrint")]      disable_print: bool,

    // Optional bits Collabora honours.
    #[serde(rename = "SupportsUpdate")]    supports_update: bool,
    #[serde(rename = "SupportsLocks")]     supports_locks: bool,
    #[serde(rename = "SupportsGetLock")]   supports_get_lock: bool,
}

pub async fn check_file_info(
    State(s): State<Arc<AppState>>,
    Path(file_id): Path<Uuid>,
    Query(q): Query<TokenQuery>,
) -> ApiResult<Json<CheckFileInfo>> {
    let claims = verify_token(&q.access_token)?;
    if claims.file_id != file_id {
        return Err(ApiError::Unauthorized);
    }

    let row: Option<(String, i64, i64, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT f.name, f.size_bytes, f.version, f.created_by, \
                COALESCE(u.display_name, 'Anonymous') \
           FROM files f LEFT JOIN users u ON u.id = f.created_by \
          WHERE f.id = $1 AND f.deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (name, size, version, owner_id, _owner_name) = row.ok_or(ApiError::NotFound)?;

    // Caller's friendly name comes from the token's user_id — that's who
    // Collabora will label edits as.  Falls back to the user id if the
    // users row was deleted.
    let caller_name: String = sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
        .bind(&claims.user_id).fetch_optional(&s.db).await?
        .unwrap_or_else(|| claims.user_id.clone());

    let write = claims.perm == "edit";
    Ok(Json(CheckFileInfo {
        base_file_name:      name,
        size,
        version:             version.to_string(),
        owner_id:            owner_id.unwrap_or_else(|| "system".into()),
        user_id:             claims.user_id,
        user_friendly_name:  caller_name,
        user_can_write:      write,
        read_only:           !write,
        disable_export:      false,
        disable_print:       false,
        supports_update:     true,
        supports_locks:      false,
        supports_get_lock:   false,
    }))
}

// -----------------------------------------------------------------------------
// GetFile — GET /wopi/files/:id/contents?access_token=…
// -----------------------------------------------------------------------------
pub async fn get_file(
    State(s): State<Arc<AppState>>,
    Path(file_id): Path<Uuid>,
    Query(q): Query<TokenQuery>,
) -> ApiResult<axum::response::Response> {
    let claims = verify_token(&q.access_token)?;
    if claims.file_id != file_id {
        return Err(ApiError::Unauthorized);
    }

    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT object_key, encrypted FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (key, encrypted) = row.ok_or(ApiError::NotFound)?;
    let (body, _) = s.storage.get(&key, encrypted).await?.ok_or(ApiError::NotFound)?;

    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, "application/octet-stream".parse().unwrap());
    h.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    Ok((StatusCode::OK, h, body).into_response())
}

// -----------------------------------------------------------------------------
// PutFile — POST /wopi/files/:id/contents?access_token=…
// -----------------------------------------------------------------------------
pub async fn put_file(
    State(s): State<Arc<AppState>>,
    Path(file_id): Path<Uuid>,
    Query(q): Query<TokenQuery>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    let claims = verify_token(&q.access_token)?;
    if claims.file_id != file_id {
        return Err(ApiError::Unauthorized);
    }
    if claims.perm != "edit" {
        // Per WOPI spec, return 404 (not 403) so Collabora downgrades to
        // view-only cleanly.  The browser's Save button gets greyed out.
        return Err(ApiError::Unauthorized);
    }

    // Pull the current file row so we know the bucket + how to encrypt the
    // new bytes.  Save edits as a NEW version so we never destroy history.
    // We also pull system_id / org_id / created_by so the quota check below
    // can enforce the same nested caps as a regular upload.
    let row: Option<(String, String, String, bool, i64, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT name, bucket, object_key, encrypted, version, file_type, \
                system_id, org_id, created_by \
           FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (name, bucket, old_key, encrypted, version, _file_type,
         system_id, org_id, created_by) = row.ok_or(ApiError::NotFound)?;

    // WOPI saves are the same as a fresh upload from a quota perspective —
    // they consume bytes and need to honour the workspace/system/org/user
    // caps.  Without this check Collabora could grow a file unbounded.
    crate::handlers::enforce_quota(
        &s,
        body.len() as i64,
        Some(&system_id),
        org_id.as_deref(),
        created_by.as_deref(),
    ).await?;

    let new_version = version + 1;
    let safe = crate::handlers::sanitize_filename(&name);
    let new_key = format!("{bucket}/{file_id}-v{new_version}-{safe}");
    let etag = s.storage.put(&new_key, body.clone(), None).await
        .map_err(ApiError::Other)?;

    // Archive the previous version in `file_versions` for the inspector
    // panel and rotation worker.
    sqlx::query(
        r#"INSERT INTO file_versions (id, file_id, version, object_key, size_bytes, etag, uploaded_by, note)
           VALUES ($1, $2, $3, $4, $5, $6, $7, 'Collabora edit')"#
    )
    .bind(Uuid::now_v7()).bind(file_id).bind(version)
    .bind(&old_key).bind(body.len() as i64).bind(&etag).bind(&claims.user_id)
    .execute(&s.db).await?;

    // Promote new bytes to the current row.
    sqlx::query(
        r#"UPDATE files SET object_key = $2, size_bytes = $3, version = $4,
                             etag = $5, encrypted = $6, modified_at = now()
            WHERE id = $1"#
    )
    .bind(file_id).bind(&new_key).bind(body.len() as i64)
    .bind(new_version).bind(&etag).bind(encrypted)
    .execute(&s.db).await?;

    // Invalidate the cached PDF preview — the next /preview hit lazy-rebuilds
    // it from the new bytes via p1::get_preview.
    sqlx::query("DELETE FROM file_previews WHERE file_id = $1")
        .bind(file_id).execute(&s.db).await.ok();

    Ok(StatusCode::OK)
}

// =============================================================================
// /api/files/:id/office-url — frontend entry point
// =============================================================================
#[derive(Debug, Serialize)]
pub struct OfficeUrl {
    /// Empty when the viewer is `disabled` or `pdf` — the frontend falls
    /// back to PDF preview in those cases.
    pub iframe_url: String,
    pub viewer: String,      // "collabora" | "pdf" | "disabled"
    pub mode:   String,      // "view" | "edit"
}

const OFFICE_TYPES: &[&str] = &["docx", "doc", "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp"];

/// Collabora's discovery.xml lists which file extensions go through which
/// URL.  We hard-code the well-known paths instead of fetching discovery
/// at runtime — Collabora's URL scheme has been stable for years and
/// embedding the fetch would add a synchronous network hop to every
/// preview request.
fn collabora_action_url(base: &str, file_type: &str, mode: &str) -> String {
    let path = match file_type {
        "docx" | "doc" | "odt"       => "/browser/dist/cool.html?WOPISrc=",
        "xlsx" | "xls" | "ods"       => "/browser/dist/cool.html?WOPISrc=",
        "pptx" | "ppt" | "odp"       => "/browser/dist/cool.html?WOPISrc=",
        _                            => "/browser/dist/cool.html?WOPISrc=",
    };
    let action = if mode == "edit" { "edit" } else { "view" };
    format!("{base}{path}__WOPI_SRC__&permission={action}&lang=en")
}

pub async fn office_url(
    State(s): State<Arc<AppState>>,
    user: crate::auth::AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<Json<OfficeUrl>> {
    let file: (String,) = sqlx::query_as(
        "SELECT file_type FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    let file_type = file.0;

    if !OFFICE_TYPES.contains(&file_type.as_str()) {
        return Err(ApiError::BadRequest("not an office document".into()));
    }

    // workspace_config decides which viewer to hand back.  This is the
    // ONLY switch — flipping it changes every file's behaviour without
    // touching code.
    let viewer: String = sqlx::query_scalar(
        "SELECT value FROM workspace_config WHERE key = 'office_viewer'"
    ).fetch_optional(&s.db).await?.unwrap_or_else(|| "pdf".into());

    if viewer != "collabora" {
        return Ok(Json(OfficeUrl { iframe_url: String::new(), viewer, mode: "view".into() }));
    }

    let base: String = sqlx::query_scalar(
        "SELECT value FROM workspace_config WHERE key = 'collabora_url'"
    ).fetch_optional(&s.db).await?.unwrap_or_else(|| "http://localhost:9980".into());

    // Anyone with editor+ can save; viewers get a read-only iframe.
    let mode = if matches!(user.0.role.as_str(), "admin" | "editor") { "edit" } else { "view" };

    // WOPI src must be a URL Collabora can reach.  In dev that means the
    // host machine's IP; in compose/k8s set BACKEND_PUBLIC_URL to whatever
    // Collabora resolves to.  We use `host.docker.internal` as the
    // sensible default for macOS/Docker Desktop.
    let backend_public = std::env::var("BACKEND_PUBLIC_URL")
        .unwrap_or_else(|_| "http://host.docker.internal:8090".into());
    let wopi_src = format!("{backend_public}/fh/wopi/files/{file_id}");

    let token = sign_token(file_id, &user.0.id, mode)?;
    // `__WOPI_SRC__` placeholder gets URL-encoded into the iframe URL.
    let iframe_url = collabora_action_url(&base, &file_type, mode)
        .replace("__WOPI_SRC__", &urlencoding::encode(&wopi_src))
        + &format!("&access_token={}", urlencoding::encode(&token));

    Ok(Json(OfficeUrl { iframe_url, viewer, mode: mode.into() }))
}

// =============================================================================
// Unit tests — token sign/verify round trip + tamper detection.
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip() {
        let id = Uuid::now_v7();
        let tok = sign_token(id, "usr_abc", "edit").unwrap();
        let claims = verify_token(&tok).unwrap();
        assert_eq!(claims.file_id, id);
        assert_eq!(claims.user_id, "usr_abc");
        assert_eq!(claims.perm, "edit");
    }

    #[test]
    fn token_tamper_rejected() {
        let id = Uuid::now_v7();
        let tok = sign_token(id, "usr_abc", "view").unwrap();
        let mut bytes: Vec<u8> = tok.into_bytes();
        // Flip the last byte of the signature.
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let bad = String::from_utf8(bytes).unwrap();
        assert!(verify_token(&bad).is_err());
    }

    #[test]
    fn token_expired_rejected() {
        // Construct a claim with exp in the past, then HMAC it manually.
        let id = Uuid::now_v7();
        let claims = TokenClaims {
            file_id: id, user_id: "u".into(), perm: "view".into(),
            exp: Utc::now().timestamp() - 10,
        };
        let payload = serde_json::to_vec(&claims).unwrap();
        let b64 = URL_SAFE_NO_PAD.encode(&payload);
        let mut mac = HmacSha256::new_from_slice(&secret()).unwrap();
        mac.update(b64.as_bytes());
        let sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        let tok = format!("{b64}.{sig}");
        assert!(verify_token(&tok).is_err());
    }
}

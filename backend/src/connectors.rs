//! Connectors — outside systems that call FileHub, set up in the admin console.
//!
//! Today one kind exists: `legacy_fileservice`, an app still calling the old
//! `/FileService/*` API. A connector holds what used to be environment variables
//! (`LEGACY_SERVICE_TOKENS`, `LEGACY_SYSTEM_MAP`, `LEGACY_DEFAULT_SYSTEM`,
//! `LEGACY_SERVICE_ROLE`), so a deployment's app names and tokens live in its
//! own database, never in code, and each app can be switched off on its own
//! once it has moved to the new API.

use std::sync::Arc;

use axum::{extract::{Path, State}, http::{header, HeaderMap, StatusCode}, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::{hash_api_key, AdminUser, User};
use crate::config::env_opt;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub const LEGACY: &str = "legacy_fileservice";
const ROLES: [&str; 2] = ["editor", "viewer"];

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Connector {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub read_only: bool,
    /// Accept callers without a registered token: calls made inside the cluster,
    /// and calls bearing a token signed by the legacy issuer (`LEGACY_JWT_KEY_URL`).
    pub trust_callers: bool,
    pub token_hint: Option<String>,
    pub path_prefixes: Vec<String>,
    pub system_id: Option<String>,
    pub org_id: Option<String>,
    pub role: String,
    pub calls: i64,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const COLS: &str = "id, name, kind, enabled, read_only, trust_callers, token_hint, path_prefixes, system_id, org_id, role, calls, last_used_at, created_at";

fn bad(m: impl Into<String>) -> ApiError { ApiError::BadRequest(m.into()) }

/// True when at least one legacy connector is switched on (the `/FileService`
/// routes answer 404 otherwise).
pub async fn legacy_on(db: &sqlx::PgPool) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM connectors WHERE kind = $1 AND enabled)")
        .bind(LEGACY).fetch_one(db).await.unwrap_or(false)
}

/// The connector whose token the caller presented, if any (and it is on).
pub async fn by_token(db: &sqlx::PgPool, headers: &HeaderMap) -> ApiResult<Option<Connector>> {
    let presented = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ")).map(str::trim).unwrap_or("");
    let mut c: Option<Connector> = if presented.is_empty() { None } else {
        sqlx::query_as(&format!("SELECT {COLS} FROM connectors WHERE token_hash = $1 AND kind = $2 AND enabled"))
            .bind(hash_api_key(presented)).bind(LEGACY).fetch_optional(db).await?
    };
    // No registered token: only a connector set to trust callers takes the
    // request, and only when it was made inside the cluster, or when it brings a
    // token signed by the legacy issuer (`LEGACY_JWT_KEY_URL`) — what the old
    // service accepted from browsers, whose pages fetch a fresh signed token
    // from their own back-end. Anything else that came through the ingress / a
    // proxy (it carries X-Forwarded-For, X-Real-IP or Forwarded) is from a
    // browser or the internet and must bring a registered token.
    if c.is_none() && (!via_proxy(headers) || (!presented.is_empty() && crate::jwks::verify_legacy(presented).await.is_some())) {
        // The call is trusted already; record it against the app it came from
        // (the uploading page's path prefix) rather than the catch-all connector,
        // so each app's calls, files and service account are its own.
        c = by_referer(db, headers).await?;
        if c.is_none() { c = by_source_header(db, headers).await?; }
        if c.is_none() {
            c = sqlx::query_as(&format!("SELECT {COLS} FROM connectors WHERE kind = $1 AND enabled AND trust_callers ORDER BY created_at LIMIT 1"))
                .bind(LEGACY).fetch_optional(db).await?;
        }
    }
    if let Some(c) = &c {
        let _ = sqlx::query("UPDATE connectors SET calls = calls + 1, last_used_at = now() WHERE id = $1").bind(&c.id).execute(db).await;
    }
    Ok(c)
}

/// The enabled connector whose path prefix matches the uploading page (`Referer`).
async fn by_referer(db: &sqlx::PgPool, headers: &HeaderMap) -> ApiResult<Option<Connector>> {
    let Some(referer) = referer(headers) else { return Ok(None) };
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.id, lower(p) FROM connectors c, unnest(c.path_prefixes) p WHERE c.kind = $1 AND c.enabled ORDER BY length(p) DESC, c.created_at",
    ).bind(LEGACY).fetch_all(db).await?;
    let Some(id) = rows.into_iter().find(|(_, p)| prefix_matches(&referer, p)).map(|(id, _)| id) else { return Ok(None) };
    Ok(sqlx::query_as(&format!("SELECT {COLS} FROM connectors WHERE id = $1")).bind(id).fetch_optional(db).await?)
}

/// The connector of the source system a caller names in `x-filehub-source`
/// (e.g. `PDP`, `C1`) — for back-ends that upload without a page `Referer`.
/// The system's page prefixes (layout sources) pick the connector.
async fn by_source_header(db: &sqlx::PgPool, headers: &HeaderMap) -> ApiResult<Option<Connector>> {
    let Some(code) = headers.get("x-filehub-source").and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()) else { return Ok(None) };
    Ok(sqlx::query_as(&format!(
        "SELECT {COLS} FROM connectors c WHERE c.kind = $1 AND c.enabled AND EXISTS (\
           SELECT 1 FROM layout_sources ls, unnest(ls.path_prefixes) p \
           WHERE upper(ls.code) = upper($2) AND lower(p) = ANY (SELECT lower(x) FROM unnest(c.path_prefixes) x)) \
         ORDER BY c.created_at LIMIT 1"
    )).bind(LEGACY).bind(code).fetch_optional(db).await?)
}

/// The lower-cased `Referer`, if any.
pub fn referer(headers: &HeaderMap) -> Option<String> {
    headers.get(header::REFERER).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()).map(str::to_lowercase)
}

/// True when `prefix` is the first path segment of the page URL `referer` (host names never match).
pub fn prefix_matches(referer: &str, prefix: &str) -> bool {
    if prefix.is_empty() { return false; }
    let path = referer.split_once("://").map(|(_, r)| r).unwrap_or(referer);
    let path = path.find('/').map(|i| &path[i..]).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    // The app is the first path segment (its context path); later segments are
    // the app's own routes (…/<app>/app/home must not match an app called "app").
    path.split('/').find(|seg| !seg.is_empty()) == Some(prefix)
}

/// First path segment of the uploading page — recorded on files whose app is not mapped yet.
pub fn referer_app(headers: &HeaderMap) -> Option<String> {
    let r = referer(headers)?;
    let path = r.split_once("://").map(|(_, r)| r).unwrap_or(&r).to_string();
    let path = path.find('/').map(|i| path[i..].to_string()).unwrap_or_default();
    path.split(['?', '#']).next().unwrap_or("").split('/').find(|s| !s.is_empty())
        .filter(|s| s.len() <= 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .map(str::to_string)
}

/// True when the request passed through the ingress or another proxy.
pub fn via_proxy(headers: &HeaderMap) -> bool {
    ["x-forwarded-for", "x-real-ip", "forwarded"].iter().any(|h| headers.contains_key(*h))
}

/// The basket for a request, from the uploading page's path prefix (`Referer`
/// …/hr-portal/… → that connector's basket). Path segments only, so a host
/// name can't match by accident.
pub async fn system_for_referer(db: &sqlx::PgPool, headers: &HeaderMap) -> Option<String> {
    let referer = referer(headers)?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT lower(p), system_id FROM connectors, unnest(path_prefixes) p WHERE kind = $1 AND enabled AND system_id IS NOT NULL ORDER BY length(p) DESC",
    ).bind(LEGACY).fetch_all(db).await.unwrap_or_default();
    rows.into_iter().find(|(p, _)| prefix_matches(&referer, p)).map(|(_, s)| s)
}

/// Which connector a request belongs to, for the call log: its token, else
/// the page it came from, else "Callers inside the cluster" for direct calls.
pub async fn name_for_request(db: &sqlx::PgPool, headers: &HeaderMap) -> Option<String> {
    if let Ok(Some(c)) = by_token(db, headers).await { return Some(c.name); }
    if let Some(r) = referer(headers) {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT lower(p), name FROM connectors, unnest(path_prefixes) p WHERE kind = $1 ORDER BY length(p) DESC",
        ).bind(LEGACY).fetch_all(db).await.unwrap_or_default();
        if let Some((_, n)) = rows.into_iter().find(|(p, _)| prefix_matches(&r, p)) { return Some(n); }
    }
    if !via_proxy(headers) {
        return sqlx::query_scalar("SELECT name FROM connectors WHERE kind = $1 AND trust_callers ORDER BY created_at LIMIT 1")
            .bind(LEGACY).fetch_optional(db).await.ok().flatten();
    }
    None
}

/// GET /api/admin/connector-calls?connector=&limit= — newest first.
#[derive(serde::Deserialize)]
pub struct CallsQuery { pub connector: Option<String>, pub limit: Option<i64>, pub offset: Option<i64> }

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct Call { pub at: chrono::DateTime<chrono::Utc>, pub connector: Option<String>, pub method: String, pub path: String, pub status: i32, pub page: Option<String>, pub client_ip: Option<String>, pub ms: Option<i32> }

pub async fn calls(
    axum::extract::State(s): axum::extract::State<std::sync::Arc<crate::AppState>>,
    user: crate::auth::AuthUser,
    axum::extract::Query(q): axum::extract::Query<CallsQuery>,
) -> ApiResult<axum::Json<Vec<Call>>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let lim = q.limit.unwrap_or(100).clamp(1, 500);
    let off = q.offset.unwrap_or(0).max(0);
    let c = q.connector.filter(|v| !v.trim().is_empty());
    Ok(axum::Json(sqlx::query_as(
        "SELECT at, connector, method, path, status, page, client_ip, ms FROM connector_calls
          WHERE ($1::text IS NULL OR connector = $1) ORDER BY at DESC LIMIT $2 OFFSET $3",
    ).bind(c).bind(lim).bind(off).fetch_all(&s.db).await?))
}

/// The account a connector acts as — one per connector, so the activity log
/// shows which app did what. Kept in step with the connector's role and basket.
pub async fn service_user(db: &sqlx::PgPool, c: &Connector) -> ApiResult<User> {
    let id = crate::new_id();
    Ok(sqlx::query_as(
        r#"INSERT INTO users (id, email, display_name, avatar_tone, password_hash, role, status, default_system_id, default_org_id, source, connector_id)
           VALUES ($1, $1 || '@connector.local', $2, 'slate', '', $3, 'active', $4, $5, 'connector', $6)
           ON CONFLICT (connector_id) WHERE connector_id IS NOT NULL DO UPDATE SET display_name = EXCLUDED.display_name, role = EXCLUDED.role,
               default_system_id = EXCLUDED.default_system_id, default_org_id = EXCLUDED.default_org_id, status = 'active'
           RETURNING *"#,
    ).bind(&id).bind(format!("Connector · {}", c.name))
     .bind(&c.role).bind(&c.system_id).bind(&c.org_id).bind(&c.id).fetch_one(db).await?)
}

/// One-time move of the old environment settings into connectors, so an
/// upgraded deployment keeps working without anyone retyping them.
pub async fn import_from_env(db: &sqlx::PgPool) {
    let any: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connectors)").fetch_one(db).await.unwrap_or(true);
    if any { return; }
    let default_sys = env_opt("LEGACY_DEFAULT_SYSTEM").or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"));
    let role = env_opt("LEGACY_SERVICE_ROLE").filter(|r| ROLES.contains(&r.as_str())).unwrap_or_else(|| "editor".into());
    let on = crate::config::env_flag("LEGACY_FILESERVICE");
    let mut n = 0;
    for pair in env_opt("LEGACY_SYSTEM_MAP").unwrap_or_default().split(',') {
        let Some((prefix, sys)) = pair.split_once('=') else { continue };
        let (prefix, sys) = (prefix.trim(), sys.trim());
        if prefix.is_empty() || sys.is_empty() { continue; }
        n += insert(db, prefix, None, vec![prefix.to_string()], Some(sys.to_string()), &role, on).await;
    }
    for (i, t) in env_opt("LEGACY_SERVICE_TOKENS").unwrap_or_default().split(',').map(str::trim).filter(|t| !t.is_empty()).enumerate() {
        n += insert(db, &format!("Service token {}", i + 1), Some(t), vec![], default_sys.clone(), &role, on).await;
    }
    // Deployments that relied on trusting every caller inside the cluster keep
    // doing so through one connector marked as such, until each app has a token.
    let trusted_before = on && env_opt("LEGACY_SERVICE_TOKENS").is_none() && env_opt("LEGACY_TRUST_CALLER").as_deref() != Some("0");
    if trusted_before || (on && n == 0) {
        let id = crate::new_id();
        let r = sqlx::query(
            "INSERT INTO connectors (id, name, kind, enabled, trust_callers, path_prefixes, system_id, role)
             VALUES ($1,'Callers inside the cluster',$2,true,$3,'{}',(SELECT id FROM systems WHERE id = $4),$5)",
        ).bind(&id).bind(LEGACY).bind(trusted_before).bind(&default_sys).bind(&role).execute(db).await;
        if r.is_ok() { n += 1; }
    }
    if n > 0 { tracing::info!("connectors: imported {n} from LEGACY_* environment settings — manage them in Admin → Connectors"); }
}

async fn insert(db: &sqlx::PgPool, name: &str, token: Option<&str>, prefixes: Vec<String>, sys: Option<String>, role: &str, on: bool) -> usize {
    let r = sqlx::query(
        "INSERT INTO connectors (id, name, kind, enabled, token_hash, token_hint, path_prefixes, system_id, role)
         VALUES ($1,$2,$3,$4,$5,$6,$7,(SELECT id FROM systems WHERE id = $8),$9)",
    ).bind(crate::new_id()).bind(name).bind(LEGACY).bind(on)
     .bind(token.map(hash_api_key)).bind(token.map(hint)).bind(&prefixes).bind(&sys).bind(role)
     .execute(db).await;
    if let Err(e) = &r { tracing::warn!("connectors: couldn't import {name}: {e}"); }
    r.is_ok() as usize
}

fn hint(t: &str) -> String { let c: Vec<char> = t.chars().collect(); c[c.len().saturating_sub(4)..].iter().collect() }

// ── Admin REST ──

#[derive(Debug, Deserialize)]
pub struct Input {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub read_only: Option<bool>,
    pub path_prefixes: Option<Vec<String>>,
    pub system_id: Option<String>,
    pub org_id: Option<String>,
    pub role: Option<String>,
    pub trust_callers: Option<bool>,
    /// The fixed token an old app already sends (it can't be changed there).
    pub token: Option<String>,
}

async fn validate(db: &sqlx::PgPool, b: &Input) -> ApiResult<()> {
    if let Some(n) = &b.name { if n.trim().is_empty() || n.chars().count() > 80 { return Err(bad("Name must be 1–80 characters.")); } }
    if let Some(t) = &b.token { if t.trim().len() < 16 { return Err(bad("The token must be at least 16 characters.")); } }
    if let Some(r) = &b.role { if !ROLES.contains(&r.as_str()) { return Err(bad("Role must be editor or viewer.")); } }
    if let Some(ps) = &b.path_prefixes {
        if ps.iter().any(|p| p.is_empty() || p.len() > 80 || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')) {
            return Err(bad("Path prefixes are single path segments: letters, digits, - _ ."));
        }
    }
    if let Some(sys) = b.system_id.as_deref().filter(|s| !s.is_empty()) {
        let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM systems WHERE id = $1 AND deleted_at IS NULL)").bind(sys).fetch_one(db).await?;
        if !ok { return Err(bad("Unknown basket.")); }
    }
    Ok(())
}

pub async fn list(State(s): State<Arc<AppState>>, _: AdminUser) -> ApiResult<Json<Vec<Connector>>> {
    Ok(Json(sqlx::query_as(&format!("SELECT {COLS} FROM connectors ORDER BY name")).fetch_all(&s.db).await?))
}

pub async fn create(State(s): State<Arc<AppState>>, admin: AdminUser, Json(b): Json<Input>) -> ApiResult<Json<Connector>> {
    validate(&s.db, &b).await?;
    let name = b.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("Give the connector a name."))?;
    let id = crate::new_id();
    sqlx::query(
        "INSERT INTO connectors (id, name, kind, enabled, read_only, path_prefixes, system_id, org_id, role, created_by, token_hash, token_hint, trust_callers)
         VALUES ($1,$2,$3,$4,$5,$6,NULLIF($7,''),NULLIF($8,''),$9,$10,$11,$12,$13)",
    ).bind(&id).bind(name).bind(LEGACY).bind(b.enabled.unwrap_or(true)).bind(b.read_only.unwrap_or(false))
     .bind(b.path_prefixes.clone().unwrap_or_default()).bind(&b.system_id).bind(&b.org_id)
     .bind(b.role.as_deref().unwrap_or("editor")).bind(&admin.0.id)
     .bind(b.token.as_deref().map(|t| hash_api_key(t.trim()))).bind(b.token.as_deref().map(|t| hint(t.trim())))
     .bind(b.trust_callers.unwrap_or(false))
     .execute(&s.db).await.map_err(token_clash)?;
    Ok(Json(sqlx::query_as(&format!("SELECT {COLS} FROM connectors WHERE id = $1")).bind(&id).fetch_one(&s.db).await?))
}

pub async fn update(State(s): State<Arc<AppState>>, _: AdminUser, Path(id): Path<String>, Json(b): Json<Input>) -> ApiResult<Json<Connector>> {
    validate(&s.db, &b).await?;
    let c: Option<Connector> = sqlx::query_as(&format!(
        "UPDATE connectors SET name = COALESCE($2, name), enabled = COALESCE($3, enabled), read_only = COALESCE($4, read_only),
                path_prefixes = COALESCE($5, path_prefixes),
                system_id = CASE WHEN $6::text IS NULL THEN system_id ELSE NULLIF($6, '') END,
                org_id = CASE WHEN $7::text IS NULL THEN org_id ELSE NULLIF($7, '') END,
                role = COALESCE($8, role), trust_callers = COALESCE($9, trust_callers)
          WHERE id = $1 RETURNING {COLS}"))
        .bind(&id).bind(b.name.as_deref().map(str::trim)).bind(b.enabled).bind(b.read_only).bind(&b.path_prefixes)
        .bind(&b.system_id).bind(&b.org_id).bind(&b.role).bind(b.trust_callers).fetch_optional(&s.db).await?;
    let mut c = c.ok_or(ApiError::NotFound)?;
    if let Some(t) = b.token.as_deref().map(str::trim) { set_token(&s.db, &id, t).await?; c.token_hint = Some(hint(t)); }
    // Keep the connector's account in step (role, basket) right away.
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE connector_id = $1)").bind(&c.id).fetch_one(&s.db).await? {
        service_user(&s.db, &c).await?;
    }
    Ok(Json(c))
}

async fn set_token(db: &sqlx::PgPool, id: &str, token: &str) -> ApiResult<()> {
    sqlx::query("UPDATE connectors SET token_hash = $2, token_hint = $3 WHERE id = $1")
        .bind(id).bind(hash_api_key(token)).bind(hint(token)).execute(db).await
        .map_err(token_clash)?;
    Ok(())
}

fn token_clash(e: sqlx::Error) -> ApiError {
    match &e { sqlx::Error::Database(d) if d.is_unique_violation() => bad("Another connector already uses this token."), _ => e.into() }
}

/// New token for the connector. Shown once; the old one stops working at once.
pub async fn rotate_token(State(s): State<Arc<AppState>>, _: AdminUser, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let token = format!("fhc_{}{}", cuid2::create_id(), cuid2::create_id());
    let n = sqlx::query("UPDATE connectors SET token_hash = $2, token_hint = $3 WHERE id = $1")
        .bind(&id).bind(hash_api_key(&token)).bind(hint(&token)).execute(&s.db).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound); }
    Ok(Json(json!({ "token": token })))
}

pub async fn remove(State(s): State<Arc<AppState>>, _: AdminUser, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let _ = sqlx::query("UPDATE users SET status = 'inactive' WHERE connector_id = $1").bind(&id).execute(&s.db).await;
    let n = sqlx::query("DELETE FROM connectors WHERE id = $1").bind(&id).execute(&s.db).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound); }
    Ok(StatusCode::NO_CONTENT)
}

/// True when `user_id` is a connector's own account.
pub async fn is_connector_user(db: &sqlx::PgPool, user_id: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND connector_id IS NOT NULL)")
        .bind(user_id).fetch_one(db).await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_match_whole_path_segments_only() {
        assert!(prefix_matches("https://h.example/app-x/app/page?x=1", "app-x"));
        assert!(!prefix_matches("https://h.example/app-x/app/page?x=1", "app"));
        assert!(prefix_matches("https://h.example/app", "app"));
        assert!(!prefix_matches("https://app.example/other", "app"));
        assert!(!prefix_matches("https://h.example/app-xy/", "app-x"));
        assert!(!prefix_matches("https://h.example/other/app/home", "app"));
    }

    #[test]
    fn referer_app_is_the_first_segment() {
        let mut h = HeaderMap::new();
        h.insert(header::REFERER, "https://h.example/My-App/x/y".parse().unwrap());
        assert_eq!(referer_app(&h).as_deref(), Some("my-app"));
    }
}

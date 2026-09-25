//! Cookie-session authentication.
//!
//! Login flow:
//!   POST /api/auth/login { email, password }
//!     → 200 + Set-Cookie: filehub_session=<token>; HttpOnly; Path=/; Max-Age=...
//!   GET  /api/auth/me        → current User
//!   POST /api/auth/logout    → expires the cookie + drops the row
//!
//! Passwords are stored as argon2id hashes.  Sessions are random 32-byte
//! tokens (URL-safe base64) kept in the `sessions` table so revocation is
//! immediate (e.g. when a user is disabled or a stolen cookie is detected).

use std::sync::Arc;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{FromRequestParts, State};
use axum::http::{request::Parts, StatusCode};
use axum::{async_trait, Json};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const COOKIE_NAME: &str = "filehub_session";
const SESSION_DAYS: i64 = 14;

// -----------------------------------------------------------------------------
// Models
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct User {
    pub id:           String,   // CUID-style (usr_admin / usr_anong / usr_viewer)
    pub email:        String,
    pub display_name: String,
    pub avatar_tone:  String,
    #[serde(skip)]
    pub password_hash: String,
    pub role:         String,
    pub status:       String,
    pub created_at:   DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: User,
}

// -----------------------------------------------------------------------------
// Password helpers
// -----------------------------------------------------------------------------
pub fn hash_password(plain: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Other(anyhow::anyhow!("hash: {e}")))
}

pub fn verify_password(plain: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    Argon2::default().verify_password(plain.as_bytes(), &parsed).is_ok()
}

fn random_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

// -----------------------------------------------------------------------------
// API keys (machine-to-machine auth) — see migrations/0015_api_keys.sql
// -----------------------------------------------------------------------------
/// Generate a fresh API key. Returns `(plaintext, sha256_hex, display_prefix)`.
/// The plaintext is shown to the caller exactly once; we persist only the hash.
pub fn generate_api_key() -> (String, String, String) {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let plaintext = format!("fhk_{}", URL_SAFE_NO_PAD.encode(bytes));
    let hash = hash_api_key(&plaintext);
    let prefix: String = plaintext.chars().take(12).collect();
    (plaintext, hash, prefix)
}

/// SHA-256 (hex) of an API key. Keys are high-entropy random tokens, so a fast
/// hash is appropriate — argon2 would only add per-request latency.
pub fn hash_api_key(key: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// Pull a bearer token out of the `Authorization` header, if present.
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = raw.strip_prefix("Bearer ").or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() { None } else { Some(token.to_string()) }
}

/// Resolve a presented API key to its service-account `User`. Rejects revoked,
/// expired, or unknown keys (and inactive users) with 401. Touches last_used_at.
pub async fn resolve_api_key(db: &PgPool, key: &str) -> Result<User, ApiError> {
    let hash = hash_api_key(key);
    let row: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT user_id, expires_at FROM api_keys WHERE key_hash = $1 AND revoked_at IS NULL",
    )
    .bind(&hash)
    .fetch_optional(db)
    .await?;
    let (user_id, expires_at) = row.ok_or(ApiError::Unauthorized)?;
    if let Some(exp) = expires_at {
        if Utc::now() > exp { return Err(ApiError::Unauthorized); }
    }
    // Fire-and-forget usage stamp — failure here must not fail the request.
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE key_hash = $1")
        .bind(&hash)
        .execute(db)
        .await;
    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1 AND status = 'active'")
        .bind(&user_id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    Ok(user)
}

// -----------------------------------------------------------------------------
// Seed bootstrap — called from AppState::init
// -----------------------------------------------------------------------------
pub(crate) const SEED_ACCOUNTS: &[(&str, &str, &str, &str, &str, &str)] = &[
    // (id, email, display_name, avatar_tone, role, password)
    ("usr_admin",  "admin@acme.go.th",  "Admin",     "indigo",  "admin",  "admin123"),
    ("usr_anong",  "anong@acme.go.th",  "Anong K.",  "rose",    "editor", "anong123"),
    ("usr_viewer", "viewer@acme.go.th", "Viewer",    "slate",   "viewer", "viewer123"),
    // Collaborators referenced throughout the seed data (org owners, activity
    // actors, comment authors, workflow reviewers). They make the workspace
    // read like a real team instead of three lonely accounts. Same dev-grade
    // passwords as above — rotate before any real deploy (see CLAUDE.md).
    ("usr_pat",    "pat@acme.go.th",    "Pat S.",    "cyan",    "editor", "pat12345"),
    ("usr_wisanu", "wisanu@acme.go.th", "Wisanu T.", "violet",  "editor", "wisanu12345"),
    ("usr_krit",   "krit@acme.go.th",   "Krit M.",   "amber",   "editor", "krit12345"),
    ("usr_sarah",  "sarah@acme.go.th",  "Sarah L.",  "emerald", "editor", "sarah12345"),
];

/// Insert any seed accounts that don't already exist.
///
/// This used to early-return whenever `users` was non-empty, which meant new
/// seed accounts could never reach an already-bootstrapped database. Instead we
/// check each account by id and insert the missing ones (`ON CONFLICT DO
/// NOTHING` guards against a concurrent insert / email collision), so adding a
/// collaborator to `SEED_ACCOUNTS` backfills it on the next boot without
/// duplicating or disturbing existing rows. Hashing only runs for accounts we
/// actually insert, so steady-state restarts stay cheap.
pub async fn bootstrap_seed_users(db: &PgPool) -> anyhow::Result<()> {
    let mut inserted = 0;
    for (id, email, name, tone, role, password) in SEED_ACCOUNTS {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
            .bind(id)
            .fetch_one(db)
            .await?;
        if exists {
            continue;
        }
        let hash = hash_password(password)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        sqlx::query(
            r#"INSERT INTO users (id, email, display_name, avatar_tone, role, password_hash)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(id)
        .bind(email)
        .bind(name)
        .bind(tone)
        .bind(role)
        .bind(hash)
        .execute(db)
        .await?;
        inserted += 1;
    }
    if inserted > 0 {
        tracing::info!("bootstrapped {inserted} seed user(s)");
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------------
pub async fn login(
    State(s): State<Arc<AppState>>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> ApiResult<(CookieJar, Json<LoginResponse>)> {
    // Disable login with the app's own accounts once identity comes from the edge
    // ⇒ "who may enter" is decided in one place (the identity provider), with no
    // back door for password guessing. (The app ships with demo accounts whose
    // passwords are in the public README.)
    if matches!(
        std::env::var("LOCAL_LOGIN").unwrap_or_default().trim(),
        "0" | "off" | "false" | "no"
    ) {
        tracing::warn!(email = %creds.email, "local login rejected: LOCAL_LOGIN is off (use single sign-on)");
        return Err(ApiError::Forbidden);
    }
    // Crude in-process throttle: cap failed-login attempts per (email, IP)
    // at ~10/min so an online bruteforce takes years instead of minutes.
    // For multi-pod deploys swap this for a Redis-backed counter, but the
    // in-process version is good enough for the single-node deploy this
    // repo targets and adds zero ops surface.
    login_throttle::check(&creds.email).await?;

    let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = $1 AND status = 'active'")
        .bind(&creds.email)
        .fetch_optional(&s.db)
        .await?;
    // Always run argon2 verify (even on unknown email) so the response time
    // is independent of whether the email exists.  Without this, an attacker
    // can enumerate registered emails just by measuring login latency.
    let (user, password_ok) = match user {
        Some(u) => {
            let ok = verify_password(&creds.password, &u.password_hash);
            (Some(u), ok)
        }
        None => {
            // Dummy verify against a constant hash so timing matches the
            // "user exists, wrong password" branch.
            let _ = verify_password(&creds.password, DUMMY_PASSWORD_HASH);
            (None, false)
        }
    };
    let Some(user) = user else {
        login_throttle::record_failure(&creds.email).await;
        return Err(ApiError::Unauthorized);
    };
    if !password_ok {
        login_throttle::record_failure(&creds.email).await;
        return Err(ApiError::Unauthorized);
    }
    login_throttle::record_success(&creds.email).await;

    let token = random_session_token();
    let expires_at = Utc::now() + Duration::days(SESSION_DAYS);
    sqlx::query(
        r#"INSERT INTO sessions (id, user_id, token, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::now_v7())
    .bind(&user.id)
    .bind(&token)
    .bind(expires_at)
    .execute(&s.db)
    .await?;

    let cookie = Cookie::build((COOKIE_NAME, token))
        .path("/")     // cover both /fh (backend) and /filehub (frontend)
        .http_only(true)
        .same_site(SameSite::Lax)
        // Mark cookies `Secure` whenever the deployment is behind HTTPS so a
        // stray http:// hop can't echo the session token in cleartext.  Driven
        // by COOKIE_SECURE=true|false so local dev over http://localhost still
        // works.  Default ON to fail-closed in production deploys that forget
        // to set the env explicitly.
        .secure(cookie_secure())
        .max_age(time::Duration::days(SESSION_DAYS))
        .build();

    Ok((jar.add(cookie), Json(LoginResponse { user })))
}

/// Read `COOKIE_SECURE`. Defaults to `true` (fail-closed). Set
/// `COOKIE_SECURE=false` in local dev when serving over plain http://.
fn cookie_secure() -> bool {
    std::env::var("COOKIE_SECURE")
        .ok()
        .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
        .unwrap_or(true)
}

pub async fn logout(
    State(s): State<Arc<AppState>>,
    jar: CookieJar,
) -> ApiResult<(CookieJar, StatusCode)> {
    if let Some(c) = jar.get(COOKIE_NAME) {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = $1")
            .bind(c.value())
            .execute(&s.db)
            .await;
    }
    let cookie = Cookie::build((COOKIE_NAME, ""))
        .path("/")
        .http_only(true)
        .secure(cookie_secure())
        .max_age(time::Duration::ZERO)
        .build();
    Ok((jar.remove(cookie), StatusCode::NO_CONTENT))
}

pub async fn me(user: AuthUser) -> Json<User> {
    Json(user.0)
}

// -----------------------------------------------------------------------------
// Auth extractor
// -----------------------------------------------------------------------------
/// Required-auth extractor — request fails with 401 when the cookie is missing
/// or the session has expired/been revoked.
/// Identity asserted by the edge (an SSO proxy such as oauth2-proxy) — the user
/// gets in without logging in again and without an API token.
///
/// Enabled only when `EDGE_AUTH_SECRET` is set, and the request must carry the
/// secret header `x-filehub-edge` matching it — the ingress / reverse proxy in
/// front of FileHub adds this header ⇒ requests sent straight into the cluster
/// cannot spoof an identity (anyone can set the x-auth-request-* headers; without
/// the matching secret they cannot be trusted at all).
///
/// Users not yet known are provisioned automatically, remembering the source
/// system/organisation (`x-filehub-system` / `x-filehub-org`) as upload defaults.
async fn edge_identity(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
) -> Result<Option<User>, ApiError> {
    let Ok(secret) = std::env::var("EDGE_AUTH_SECRET") else { return Ok(None) };
    if secret.trim().is_empty() {
        return Ok(None);
    }
    let presented = headers.get("x-filehub-edge").and_then(|v| v.to_str().ok()).unwrap_or("");
    if presented != secret {
        return Ok(None);
    }
    // Identity can arrive two ways: plain oauth2-proxy headers (auth_request mode)
    // or the USERINFO claim inside the access token forwarded by the gateway.
    let ui = edge_userinfo(headers);
    let email = headers
        .get("x-auth-request-email")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .or_else(|| ui.as_ref().and_then(|u| u.email.clone()).map(|e| e.to_lowercase()));
    let Some(email) = email else { return Ok(None) };

    // ── Role gate: who may use the file hub ───────────────────────────────
    // EDGE_REQUIRED_ROLES = identity-provider role codes allowed in (comma-separated).
    // Unset = open to everyone signed in via SSO (the original behaviour).
    // Users without a matching role get a clear 403, not a blank page or a
    // confusing bounce to the login screen.
    let user_roles: Vec<String> = ui.as_ref().map(|u| u.roles.clone()).unwrap_or_default();
    let required = role_list("EDGE_REQUIRED_ROLES");
    if !required.is_empty() && !has_any(&user_roles, &required) {
        tracing::warn!(
            email = %email, roles = ?user_roles,
            "file hub access denied — user has none of the roles in EDGE_REQUIRED_ROLES"
        );
        return Err(ApiError::Forbidden);
    }
    // Admin rights are also mapped from identity-provider roles, so they are not maintained in two places.
    let admin_roles = role_list("EDGE_ADMIN_ROLES");
    let is_admin = !admin_roles.is_empty() && has_any(&user_roles, &admin_roles);

    let system_id = header_str(headers, "x-filehub-system")
        .or_else(|| std::env::var("EDGE_DEFAULT_SYSTEM").ok());
    // The user's organisation — from the identity-provider claim first, else from the header.
    let org_id = resolve_org(
        state,
        system_id.as_deref(),
        ui.as_ref()
            .and_then(|u| u.agency_code.clone())
            .or_else(|| header_str(headers, "x-filehub-org")),
        ui.as_ref().and_then(|u| u.agency_name.clone()),
    )
    .await?;

    if let Some(u) = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE email = $1 AND status = 'active'",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?
    {
        // Existing users who moved organisation (or accounts created before their
        // organisation was known) must always get the current one, otherwise their
        // files land in the old organisation.
        if org_id.is_some() {
            sqlx::query(
                "UPDATE users SET default_org_id = $1, default_system_id = COALESCE(default_system_id, $2)                  WHERE id = $3 AND default_org_id IS DISTINCT FROM $1",
            )
            .bind(&org_id)
            .bind(&system_id)
            .bind(&u.id)
            .execute(&state.db)
            .await?;
        }
        // Role changes in the identity provider take effect here immediately — no manual sync.
        if u.password_hash.is_empty() && ((is_admin && u.role != "admin") || (!is_admin && !admin_roles.is_empty() && u.role == "admin")) {
            let want: String = if is_admin { "admin".into() } else { std::env::var("EDGE_AUTH_ROLE").unwrap_or_else(|_| "editor".into()) };
            sqlx::query("UPDATE users SET role = $1 WHERE id = $2")
                .bind(&want)
                .bind(&u.id)
                .execute(&state.db)
                .await?;
            return Ok(sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
                .bind(&u.id)
                .fetch_optional(&state.db)
                .await?);
        }
        return Ok(Some(u));
    }

    // Auto-provision the account — no password (reachable through the edge only).
    let display = ui
        .as_ref()
        .and_then(|u| u.full_name.clone())
        .or_else(|| header_str(headers, "x-auth-request-preferred-username"))
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user").to_string());
    let role = if is_admin {
        "admin".to_string()
    } else {
        std::env::var("EDGE_AUTH_ROLE").unwrap_or_else(|_| "editor".into())
    };
    let id = format!("usr_edge_{}", uuid::Uuid::now_v7().simple());

    sqlx::query(
        r#"INSERT INTO users (id, email, display_name, avatar_tone, password_hash, role, status,
                              default_system_id, default_org_id, source)
           VALUES ($1, $2, $3, 'slate', '', $4, 'active', $5, $6, 'edge')"#,
    )
    .bind(&id)
    .bind(&email)
    .bind(&display)
    .bind(&role)
    .bind(&system_id)
    .bind(&org_id)
    .execute(&state.db)
    .await?;

    let u: User = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    tracing::info!(email = %u.email, role = %u.role, "edge identity: provisioned user");
    Ok(Some(u))
}

fn header_str(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Look up the org by the supplied organisation code, creating it if missing
/// (name = code until someone renames it). Returns None when no organisation is
/// given or the system is not yet known.
async fn resolve_org(
    state: &Arc<AppState>,
    system_id: Option<&str>,
    org_code: Option<String>,
    org_name: Option<String>,
) -> Result<Option<String>, ApiError> {
    let (Some(system_id), Some(code)) = (system_id, org_code) else { return Ok(None) };
    let name = org_name.unwrap_or_else(|| code.clone());
    if let Some((id,)) = sqlx::query_as::<_, (String,)>("SELECT id FROM orgs WHERE code = $1")
        .bind(&code)
        .fetch_optional(&state.db)
        .await?
    {
        return Ok(Some(id));
    }
    let id = format!("org_{}", code.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "_"));
    sqlx::query(
        r#"INSERT INTO orgs (id, system_id, name, code, tier, owner, status)
           VALUES ($1, $2, $3, $4, 'standard', 'auto', 'Active')
           ON CONFLICT (code) DO NOTHING"#,
    )
    .bind(&id)
    .bind(system_id)
    .bind(&name)
    .bind(&code)
    .execute(&state.db)
    .await?;
    Ok(Some(id))
}

/// Identity supplied by the edge — decoded from `X-Forwarded-Access-Token`.
///
/// The gateway (oauth2-proxy) forwards the Keycloak access token in this header,
/// and the identity provider may embed a `USERINFO` claim carrying the email,
/// full name and **home organisation** ⇒ FileHub knows who is uploading and for
/// which organisation without anyone filling it in.
///
/// The token signature is not verified here, because the gate is the secret
/// `x-filehub-edge` header added by the ingress — requests that did not pass the
/// edge lack the matching secret. (Verifying here is possible but would require
/// fetching and caching Keycloak's JWKS, which is not needed yet.)
pub(crate) struct EdgeUserInfo {
    pub email: Option<String>,
    pub full_name: Option<String>,
    pub agency_code: Option<String>,
    pub agency_name: Option<String>,
    /// All of the user's role codes — from the `USERROLE` claim embedded in the token
    /// (Keycloak's own roles in realm_access.roles are merged in here too).
    pub roles: Vec<String>,
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
}

pub(crate) fn edge_userinfo(headers: &axum::http::HeaderMap) -> Option<EdgeUserInfo> {
    let token = header_str(headers, "x-forwarded-access-token")?;
    let payload = token.split('.').nth(1)?;
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    // USERINFO may be an object or a JSON string, depending on the claim mapper.
    let ui = match claims.get("USERINFO") {
        Some(serde_json::Value::String(s)) => serde_json::from_str(s).ok()?,
        Some(v) => v.clone(),
        None => claims.clone(),
    };
    // Roles: USERROLE may be an array or a JSON string.
    let mut roles: Vec<String> = Vec::new();
    match claims.get("USERROLE") {
        Some(serde_json::Value::Array(a)) => {
            for v in a { if let Some(x) = v.as_str() { roles.push(x.trim().to_string()); } }
        }
        Some(serde_json::Value::String(raw)) => {
            if let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(raw) {
                for v in a { if let Some(x) = v.as_str() { roles.push(x.trim().to_string()); } }
            } else {
                roles.push(raw.trim().to_string());
            }
        }
        _ => {}
    }
    if let Some(a) = claims
        .get("realm_access")
        .and_then(|r| r.get("roles"))
        .and_then(|r| r.as_array())
    {
        for v in a { if let Some(x) = v.as_str() { roles.push(x.trim().to_string()); } }
    }
    roles.retain(|r| !r.is_empty());

    Some(EdgeUserInfo {
        email: json_str(&ui, "EMAIL").or_else(|| json_str(&claims, "email")),
        full_name: json_str(&ui, "FULL_NAME")
            .or_else(|| json_str(&claims, "name"))
            .or_else(|| json_str(&claims, "preferred_username")),
        agency_code: json_str(&ui, "AGENCY_CODE"),
        agency_name: json_str(&ui, "AGENCY_NAME"),
        roles,
    })
}

/// Role codes read from a comma-separated env var — unset = allow everyone signed in via SSO.
fn role_list(key: &str) -> Vec<String> {
    std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .collect()
}

fn has_any(user_roles: &[String], wanted: &[String]) -> bool {
    user_roles.iter().any(|r| wanted.contains(&r.to_lowercase()))
}

pub struct AuthUser(pub User);

#[async_trait]
impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // Bearer API key takes precedence — lets headless callers authenticate
        // without a browser session (see resolve_api_key).
        if let Some(key) = bearer_token(&parts.headers) {
            return Ok(AuthUser(resolve_api_key(&state.db, &key).await?));
        }
        // FileHub's own session cookie wins over the edge identity — an admin who
        // logged into the console must act as that admin account, not be overridden
        // by the SSO identity of the same browser.
        let jar = CookieJar::from_headers(&parts.headers);
        let Some(token) = jar.get(COOKIE_NAME).map(|c| c.value().to_string()) else {
            // No cookie = an API call from an integrated application ⇒ use the edge (SSO) identity.
            if let Some(u) = edge_identity(state, &parts.headers).await? {
                return Ok(AuthUser(u));
            }
            return Err(ApiError::Unauthorized);
        };

        let row: Option<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT user_id, expires_at FROM sessions WHERE token = $1",
        )
        .bind(&token)
        .fetch_optional(&state.db)
        .await?;
        let (user_id, expires_at) = row.ok_or(ApiError::Unauthorized)?;
        if Utc::now() > expires_at {
            return Err(ApiError::Unauthorized);
        }

        // Touch last_seen_at — fire-and-forget; failures here don't kill the request.
        let _ = sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE token = $1")
            .bind(&token)
            .execute(&state.db)
            .await;

        let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1 AND status = 'active'")
            .bind(&user_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        Ok(AuthUser(user))
    }
}

/// Optional-auth extractor — `None` when the user isn't logged in.  Useful
/// for endpoints that change behaviour based on identity without requiring it
/// (e.g. activity feed, search).
pub struct MaybeAuthUser(pub Option<User>);

#[async_trait]
impl FromRequestParts<Arc<AppState>> for MaybeAuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(u)  => Ok(MaybeAuthUser(Some(u.0))),
            Err(_) => Ok(MaybeAuthUser(None)),
        }
    }
}

/// Role gate helper — call this at the top of mutating handlers that need
/// admin/editor privileges.
pub fn require_role(user: &User, allowed: &[&str]) -> Result<(), ApiError> {
    if allowed.contains(&user.role.as_str()) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// Verify the caller can see / write to a given system.  Returns 403 if a
/// non-admin tries to touch someone else's personal drive, 404 if the system
/// id doesn't exist (or is soft-deleted).
///
/// Use this on every handler that takes a system_id from the caller, so a
/// signed-in viewer can't reach into `sys_personal_<other>` by typing its id
/// into the URL.  Admins bypass for support / forensics workflows.
pub async fn ensure_system_access(
    db: &PgPool,
    user: &User,
    system_id: &str,
) -> Result<(), ApiError> {
    if user.role == "admin" { return Ok(()); }
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT system_type, owner_user_id FROM systems \
         WHERE id = $1 AND deleted_at IS NULL"
    ).bind(system_id).fetch_optional(db).await?;
    // System doesn't exist → don't shadow the handler's own "unknown system"
    // error path.  Each caller already maps this to the right status code
    // (400 BadRequest for write paths, empty result for list paths) and
    // tests rely on that semantic.  We only return Forbidden here when the
    // system *does* exist but the caller doesn't own it.
    let Some((system_type, owner)) = row else { return Ok(()); };
    if system_type == "personal" && owner.as_deref() != Some(&user.id) {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

/// Build the list of system ids the caller can read from.  Used to scope
/// listings (e.g. `list_files`, `search_files`) so a viewer doesn't see
/// other users' personal-drive contents in unfiltered queries.
///
/// Returns `None` for admins (= no scoping, see everything) and `Some(vec)`
/// for everyone else (all shared systems + the caller's personal drive id).
pub async fn effective_system_ids(
    db: &PgPool,
    user: &User,
) -> Result<Option<Vec<String>>, ApiError> {
    if user.role == "admin" { return Ok(None); }
    let ids: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM systems \
         WHERE deleted_at IS NULL AND (system_type = 'shared' OR owner_user_id = $1)"
    ).bind(&user.id).fetch_all(db).await?;
    Ok(Some(ids.into_iter().map(|(id,)| id).collect()))
}

/// Tower middleware that rejects every request lacking a valid
/// `filehub_session` cookie with 401.  Mounted on the *private* sub-router
/// only — `/api/auth/login`, `/api/health`, `/api/share/:token/*`, and
/// `/wopi/*` deliberately sit outside this layer because the token IS the
/// auth (share-link token / WOPI access_token) or the route is harmless
/// (health probe).
///
/// History note: we used to rely on per-handler `AuthUser` extractors for
/// this, but ~half of the read endpoints (list_files, stats, list_activity,
/// list_folders, …) had no extractor at all and silently served data to
/// anonymous callers.  A global gate is harder to forget than 30 individual
/// type annotations.
///
/// The actual `User` row is still fetched per-handler by `AuthUser` when a
/// handler needs the identity — this middleware just enforces "you have a
/// live session" before the handler runs.  Session-touch (`last_seen_at`)
/// also stays in the extractor so we don't burn an extra UPDATE on the
/// many routes that don't care who the caller is.
pub async fn require_session(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, ApiError> {
    // Bearer API key — validate it here so headless callers pass the gate too.
    if let Some(key) = bearer_token(req.headers()) {
        resolve_api_key(&state.db, &key).await?;
        return Ok(next.run(req).await);
    }
    // Same order as AuthUser: FileHub's cookie first, then the edge (SSO) identity.
    // (If edge identities were not let through this gate, every endpoint behind it
    //  would answer 401 while `/api/auth/me`, outside the gate, says the user is
    //  known — leaving every page blank.)
    let jar = CookieJar::from_headers(req.headers());
    let Some(token) = jar.get(COOKIE_NAME).map(|c| c.value().to_string()) else {
        if edge_identity(&state, req.headers()).await?.is_some() {
            return Ok(next.run(req).await);
        }
        return Err(ApiError::Unauthorized);
    };

    let row: Option<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT user_id, expires_at FROM sessions WHERE token = $1",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await?;
    let (_user_id, expires_at) = row.ok_or(ApiError::Unauthorized)?;
    if Utc::now() > expires_at {
        return Err(ApiError::Unauthorized);
    }
    Ok(next.run(req).await)
}

// =============================================================================
// Login throttling
// =============================================================================
//
// In-process token bucket keyed by email.  Caps failed logins at 10 per
// rolling minute; after 10 misses the email is locked out for 60 s.  This
// is intentionally simple — for multi-pod deploys swap the static for a
// Redis counter, but the single-node deploy this repo targets needs no
// extra ops surface to get useful protection.

/// Frozen argon2id hash of a constant password.  Used as a side-channel
/// equaliser when the email lookup miss — running verify against this keeps
/// response time independent of whether the email exists.
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$bF93eVNDc1RmcDdLZG5pTQ$kE0n/4f6Aw0NSnVxLPq8fEPLQp9lyIULqYO0EJ3pEsM";

mod login_throttle {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    use chrono::{DateTime, Utc};

    use crate::error::ApiError;

    const MAX_FAILS_PER_MINUTE: u32 = 10;
    const LOCKOUT_SECS:         i64 = 60;

    #[derive(Default)]
    struct State {
        // email -> (window_start, fails_in_window, lockout_until)
        entries: HashMap<String, (DateTime<Utc>, u32, Option<DateTime<Utc>>)>,
    }

    fn state() -> &'static Mutex<State> {
        static S: OnceLock<Mutex<State>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(State::default()))
    }

    pub async fn check(email: &str) -> Result<(), ApiError> {
        let now = Utc::now();
        let mut st = state().lock().await;
        if let Some((_, _, Some(lock))) = st.entries.get(email) {
            if now < *lock {
                return Err(ApiError::TooManyRequests(
                    "too many login attempts — try again shortly".into()
                ));
            }
        }
        // Garbage-collect expired entries opportunistically so a long-running
        // process doesn't keep historical state forever.
        st.entries.retain(|_, (ws, _, lock)| {
            (now - *ws).num_seconds() < 600 || lock.map(|l| l > now).unwrap_or(false)
        });
        Ok(())
    }

    pub async fn record_failure(email: &str) {
        let now = Utc::now();
        let mut st = state().lock().await;
        let e = st.entries.entry(email.to_string()).or_insert((now, 0, None));
        if (now - e.0).num_seconds() >= 60 { e.0 = now; e.1 = 0; e.2 = None; }
        e.1 += 1;
        if e.1 >= MAX_FAILS_PER_MINUTE {
            e.2 = Some(now + chrono::Duration::seconds(LOCKOUT_SECS));
        }
    }

    pub async fn record_success(email: &str) {
        let mut st = state().lock().await;
        st.entries.remove(email);
    }
}

// =============================================================================
// Upload rate limiting
// =============================================================================
//
// In-process per-user cap on file uploads (regular POST /api/files +
// TUS-session create POST /api/uploads).  Caps starts at 60 per rolling
// minute, returning 429 once exceeded.  Mirrors the `login_throttle` design
// above: a keyed counter behind a `OnceLock<Mutex<..>>`.
//
// NOTE: per-process only — a multi-pod deploy needs a shared store (Redis)
// to enforce the cap cluster-wide, exactly like the login throttle.

/// Charge one upload against `user_id`'s budget.  Returns `Err(TooManyRequests)`
/// once the caller exceeds `MAX_UPLOADS_PER_MINUTE` within the rolling window.
/// Call this at the top of upload handlers, *after* authentication.
pub async fn upload_rate_limit(user_id: &str) -> Result<(), ApiError> {
    upload_throttle::check_and_record(user_id, upload_throttle::MAX_UPLOADS_PER_MINUTE).await
}

/// Rate limit for callers that share one service identity (the legacy
/// `/FileService` account shared by every legacy caller).  Keyed by the calling
/// system instead of the shared user, with its own ceiling
/// (`LEGACY_UPLOAD_RATE_PER_MIN`, default 1200/min per pod) — otherwise all
/// modules together were capped at 60 uploads per minute.
pub async fn upload_rate_limit_keyed(key: &str, per_minute: u32) -> Result<(), ApiError> {
    upload_throttle::check_and_record(key, per_minute).await
}

mod upload_throttle {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    use chrono::{DateTime, Utc};

    use crate::error::ApiError;

    pub const MAX_UPLOADS_PER_MINUTE: u32 = 60;

    #[derive(Default)]
    struct State {
        // user_id -> (window_start, uploads_in_window)
        entries: HashMap<String, (DateTime<Utc>, u32)>,
    }

    fn state() -> &'static Mutex<State> {
        static S: OnceLock<Mutex<State>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(State::default()))
    }

    pub async fn check_and_record(user_id: &str, max_per_minute: u32) -> Result<(), ApiError> {
        let now = Utc::now();
        let mut st = state().lock().await;
        // Drop stale windows so a long-running process doesn't accumulate
        // state for every user it has ever served.
        st.entries.retain(|_, (ws, _)| (now - *ws).num_seconds() < 60);
        let e = st.entries.entry(user_id.to_string()).or_insert((now, 0));
        if (now - e.0).num_seconds() >= 60 { e.0 = now; e.1 = 0; }
        if e.1 >= max_per_minute {
            return Err(ApiError::TooManyRequests(
                "upload rate limit exceeded — slow down".into()
            ));
        }
        e.1 += 1;
        Ok(())
    }
}

// =============================================================================
// Unit tests
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_round_trip() {
        let hash = hash_password("hunter2").unwrap();
        assert!(verify_password("hunter2", &hash));
        assert!(!verify_password("hunter3", &hash));
        assert!(!verify_password("",        &hash));
    }

    #[test]
    fn random_token_is_url_safe_and_long() {
        let t = random_session_token();
        assert!(t.len() >= 42);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn require_role_enforces_allowlist() {
        let admin  = User { id: "usr_test".into(), email: "".into(), display_name: "".into(), avatar_tone: "".into(), password_hash: "".into(), role: "admin".into(),  status: "active".into(), created_at: Utc::now() };
        let viewer = User { id: "usr_test".into(), email: "".into(), display_name: "".into(), avatar_tone: "".into(), password_hash: "".into(), role: "viewer".into(), status: "active".into(), created_at: Utc::now() };
        assert!(require_role(&admin,  &["admin"]).is_ok());
        assert!(require_role(&viewer, &["admin"]).is_err());
        assert!(require_role(&viewer, &["admin", "viewer"]).is_ok());
    }
}

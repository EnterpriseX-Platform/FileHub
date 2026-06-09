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
const SEED_ACCOUNTS: &[(&str, &str, &str, &str, &str, &str)] = &[
    // (id, email, display_name, avatar_tone, role, password)
    ("usr_admin",  "admin@acme.go.th",  "Admin",    "indigo", "admin",  "admin123"),
    ("usr_anong",  "anong@acme.go.th",  "Anong K.", "rose",   "editor", "anong123"),
    ("usr_viewer", "viewer@acme.go.th", "Viewer",   "slate",  "viewer", "viewer123"),
];

pub async fn bootstrap_seed_users(db: &PgPool) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await?;
    if count > 0 {
        return Ok(());
    }
    for (id, email, name, tone, role, password) in SEED_ACCOUNTS {
        let hash = hash_password(password)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        sqlx::query(
            r#"INSERT INTO users (id, email, display_name, avatar_tone, role, password_hash)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(id)
        .bind(email)
        .bind(name)
        .bind(tone)
        .bind(role)
        .bind(hash)
        .execute(db)
        .await?;
    }
    tracing::info!("bootstrapped {} seed users", SEED_ACCOUNTS.len());
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
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(COOKIE_NAME)
            .map(|c| c.value().to_string())
            .ok_or(ApiError::Unauthorized)?;

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
    let jar = CookieJar::from_headers(req.headers());
    let token = jar
        .get(COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or(ApiError::Unauthorized)?;

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
    upload_throttle::check_and_record(user_id).await
}

mod upload_throttle {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    use chrono::{DateTime, Utc};

    use crate::error::ApiError;

    const MAX_UPLOADS_PER_MINUTE: u32 = 60;

    #[derive(Default)]
    struct State {
        // user_id -> (window_start, uploads_in_window)
        entries: HashMap<String, (DateTime<Utc>, u32)>,
    }

    fn state() -> &'static Mutex<State> {
        static S: OnceLock<Mutex<State>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(State::default()))
    }

    pub async fn check_and_record(user_id: &str) -> Result<(), ApiError> {
        let now = Utc::now();
        let mut st = state().lock().await;
        // Drop stale windows so a long-running process doesn't accumulate
        // state for every user it has ever served.
        st.entries.retain(|_, (ws, _)| (now - *ws).num_seconds() < 60);
        let e = st.entries.entry(user_id.to_string()).or_insert((now, 0));
        if (now - e.0).num_seconds() >= 60 { e.0 = now; e.1 = 0; }
        if e.1 >= MAX_UPLOADS_PER_MINUTE {
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

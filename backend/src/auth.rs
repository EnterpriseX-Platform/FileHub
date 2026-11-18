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

/// A bearer credential: a FileHub API key (`fhk_…`) or an IAMX access token.
///
/// IAMX tokens let another system call FileHub **as the signed-in person**
/// (their access, their agency, their name on the file) by forwarding the
/// token it already holds — no FileHub key, no second login. The token must
/// verify against IAMX's keys (`IAMX_JWKS_URL`, falling back to `EDGE_JWKS_URL`);
/// with neither set, IAMX tokens are refused. `IAMX_ALLOWED_CLIENTS` (optional,
/// comma-separated) limits which client apps (`azp`) may do this.
/// `X-FileHub-System` / `X-FileHub-Org` may say which basket/agency the calling
/// system uploads to by default, as with the edge.
pub async fn bearer_user(state: &Arc<AppState>, headers: &axum::http::HeaderMap, key: &str) -> Result<User, ApiError> {
    if key.starts_with("fhk_") || key.matches('.').count() != 2 {
        return resolve_api_key(&state.db, key).await;
    }
    let url = std::env::var("IAMX_JWKS_URL").ok().filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("EDGE_JWKS_URL").ok().filter(|v| !v.trim().is_empty()));
    let Some(url) = url else {
        tracing::warn!("IAMX bearer token refused: IAMX_JWKS_URL is not set");
        return Err(ApiError::Unauthorized);
    };
    let Some(claims) = crate::jwks::verify_with(&url, key).await else { return Err(ApiError::Unauthorized) };
    // Any client in the realm can mint a user token, so only the listed ones count.
    let allowed = role_list("IAMX_ALLOWED_CLIENTS");
    let azp = claims.get("azp").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let aud: Vec<String> = match claims.get("aud") {
        Some(serde_json::Value::String(a)) => vec![a.to_lowercase()],
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str()).map(str::to_lowercase).collect(),
        _ => vec![],
    };
    if allowed.is_empty() || !(allowed.contains(&azp) || aud.iter().any(|a| allowed.contains(a))) {
        tracing::warn!(azp = %azp, "IAMX bearer token refused: client not in IAMX_ALLOWED_CLIENTS");
        return Err(ApiError::Forbidden);
    }
    if claims.get("email_verified").and_then(|v| v.as_bool()) == Some(false) {
        tracing::warn!("IAMX bearer token refused: email not verified");
        return Err(ApiError::Forbidden);
    }
    // Only the verified token and the basket hint. The agency comes from the
    // signed claim alone — a caller-set header would re-home them into any agency.
    let mut h = axum::http::HeaderMap::new();
    h.insert("x-forwarded-access-token", axum::http::HeaderValue::from_str(key).map_err(|_| ApiError::Unauthorized)?);
    if let Some(v) = headers.get("x-filehub-system") { h.insert("x-filehub-system", v.clone()); }
    identity_from_trusted(state, &h).await?.ok_or(ApiError::Unauthorized)
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
    for (_, email, name, tone, role, password) in SEED_ACCOUNTS {
        let id = crate::new_id();
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
            .bind(email)
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
        .bind(&id)
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
    // At most 10 sign-in attempts per email per minute, counted across pods.
    // Counted before the password check so parallel guesses can't race past it;
    // a successful sign-in clears the count.
    let throttle_key = format!("login:{}", creds.email.trim().to_lowercase());
    crate::ratelimit::hit(&s.db, &throttle_key, LOGIN_FAILS_PER_MINUTE, "Too many sign-in attempts. Try again in a minute.").await?;

    // Active accounts sign in; a self sign-up still waiting for approval is told
    // so (only after its own password checks out, so nothing leaks to others).
    let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = $1 AND status IN ('active', 'pending')")
        .bind(&creds.email)
        .fetch_optional(&s.db)
        .await?;
    // Always run argon2 verify (even on unknown email) so the response time
    // is independent of whether the email exists.  Without this, an attacker
    // can enumerate registered emails just by measuring login latency.
    // Argon2 is deliberately slow: run it off the async workers so a burst of
    // sign-ins doesn't stall every other request. Unknown emails verify against
    // a dummy hash so timing doesn't reveal which emails exist.
    let hash = user.as_ref().map(|u| u.password_hash.clone()).unwrap_or_else(|| DUMMY_PASSWORD_HASH.to_string());
    let password = creds.password.clone();
    let verified = tokio::task::spawn_blocking(move || verify_password(&password, &hash)).await.unwrap_or(false);
    let password_ok = user.is_some() && verified;
    let user = match user {
        Some(u) if password_ok && u.status == "pending" =>
            return Err(ApiError::BadRequest("Your account is waiting for an administrator to turn it on.".into())),
        Some(u) if password_ok => u,
        _ => return Err(ApiError::Unauthorized),
    };
    crate::ratelimit::reset(&s.db, &throttle_key).await;

    let token = random_session_token();
    let expires_at = Utc::now() + Duration::days(SESSION_DAYS);
    sqlx::query(
        r#"INSERT INTO sessions (id, user_id, token, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(crate::Id::new())
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
    let cookie = with_cookie_domain(cookie);

    Ok((jar.add(cookie), Json(LoginResponse { user })))
}

/// `COOKIE_DOMAIN` (e.g. `.example.go.th`): share the session with a separate
/// upload host (`UPLOAD_BASE_URL`) under the same parent domain. Unset = host only.
fn with_cookie_domain(mut c: Cookie<'static>) -> Cookie<'static> {
    if let Some(d) = std::env::var("COOKIE_DOMAIN").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        c.set_domain(d);
    }
    c
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
    let cookie = with_cookie_domain(cookie);
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
/// The edge (SSO) identity alone — for routes whose callers also send a token
/// FileHub can't verify (legacy FileService), so the person still wins.
pub(crate) async fn edge_user(state: &Arc<AppState>, headers: &axum::http::HeaderMap) -> Result<Option<User>, ApiError> {
    edge_identity(state, headers).await
}

async fn edge_identity(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
) -> Result<Option<User>, ApiError> {
    let Ok(secret) = std::env::var("EDGE_AUTH_SECRET") else { return Ok(None) };
    if secret.trim().is_empty() {
        return Ok(None);
    }
    let presented = headers.get("x-filehub-edge").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !crate::config::constant_eq(presented.as_bytes(), secret.as_bytes()) {
        return Ok(None);
    }
    identity_from_trusted(state, headers).await
}

/// Map an identity we already trust (edge secret matched, or a verified IAMX
/// token) to a FileHub user — find by IAMX `sub`, then email; keep roles and
/// agency in step; create the account on first sight.
async fn identity_from_trusted(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
) -> Result<Option<User>, ApiError> {
    // Defence in depth: when EDGE_JWKS_URL is set the forwarded token must carry a
    // valid signature from the identity provider, not just arrive with the secret.
    if let Some(tok) = header_str(headers, "x-forwarded-access-token") {
        if !crate::jwks::verify_if_configured(&tok).await {
            tracing::warn!("edge identity rejected: access token signature/expiry check failed");
            return Err(ApiError::Unauthorized);
        }
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

    // IAMX subject first (email can change), then email for accounts made before
    // the subject was recorded.
    let subject = ui.as_ref().and_then(|u| u.subject.clone());
    let by_subject = match &subject {
        Some(sub) => sqlx::query_as::<_, User>("SELECT * FROM users WHERE iamx_subject = $1 AND status = 'active'")
            .bind(sub).fetch_optional(&state.db).await?,
        None => None,
    };
    let found = match by_subject {
        Some(u) => Some(u),
        None => {
            let by_email = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1 AND status = 'active'")
                .bind(&email).fetch_optional(&state.db).await?;
            // A local password account is never taken over by a single-sign-on
            // identity that merely carries the same email.
            if by_email.as_ref().is_some_and(|u| !u.password_hash.is_empty()) {
                tracing::warn!(email = %email, "SSO identity refused: email belongs to a local account");
                return Err(ApiError::Forbidden);
            }
            by_email
        }
    };
    if let Some(u) = &found {
        let roles_upper: Vec<String> = user_roles.iter().map(|r| r.to_uppercase()).collect();
        sqlx::query(
            "UPDATE users SET iamx_subject = COALESCE($1, iamx_subject), iamx_roles = $2, email = $3 WHERE id = $4",
        ).bind(&subject).bind(&roles_upper).bind(&email).bind(&u.id).execute(&state.db).await?;
    }
    if let Some(u) = found
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
        // Email and roles were just refreshed from the token: answer with them.
        return Ok(Some(User { email: email.clone(), ..u }));
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
    let id = crate::new_id();

    let roles_upper: Vec<String> = user_roles.iter().map(|r| r.to_uppercase()).collect();
    sqlx::query(
        r#"INSERT INTO users (id, email, display_name, avatar_tone, password_hash, role, status,
                              default_system_id, default_org_id, source, iamx_subject, iamx_roles)
           VALUES ($1, $2, $3, 'slate', '', $4, 'active', $5, $6, 'edge', $7, $8)"#,
    )
    .bind(&id)
    .bind(&email)
    .bind(&display)
    .bind(&role)
    .bind(&system_id)
    .bind(&org_id)
    .bind(&subject)
    .bind(&roles_upper)
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
    let id = crate::new_id();
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
    /// IAMX / Keycloak subject (`sub`) — the stable key for the person.
    pub subject: Option<String>,
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
        subject: json_str(&claims, "sub").or_else(|| json_str(&ui, "USER_ID")),
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
        // Behind `require_session` the user was already resolved for this request.
        if let Some(u) = parts.extensions.get::<User>() { return Ok(AuthUser(u.clone())); }
        resolve_user(state, &parts.headers).await?.map(AuthUser).ok_or(ApiError::Unauthorized)
    }
}

/// Who is calling, in this order: a bearer credential (API key or IAMX token),
/// FileHub's own session cookie (an admin signed into the console acts as that
/// account, not as the SSO identity of the same browser), then the edge's SSO
/// identity. `None` = anonymous.
pub async fn resolve_user(state: &Arc<AppState>, headers: &axum::http::HeaderMap) -> Result<Option<User>, ApiError> {
    if let Some(key) = bearer_token(headers) {
        return Ok(Some(bearer_user(state, headers, &key).await?));
    }
    let jar = CookieJar::from_headers(headers);
    let Some(token) = jar.get(COOKIE_NAME).map(|c| c.value().to_string()) else {
        return edge_identity(state, headers).await;
    };
    // Session + person in one round trip, plus whether "last seen" is due.
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT u.*, s.last_seen_at < now() - interval '60 seconds' AS seen_due
           FROM sessions s JOIN users u ON u.id = s.user_id
          WHERE s.token = $1 AND s.expires_at > now() AND u.status = 'active'",
    ).bind(&token).fetch_optional(&state.db).await?.ok_or(ApiError::Unauthorized)?;
    let user = User::from_row(&row)?;
    // "Last seen" at most once a minute, off the request path.
    if row.try_get::<bool, _>("seen_due").unwrap_or(false) {
        let db = state.db.clone();
        tokio::spawn(async move {
            let _ = sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE token = $1").bind(&token).execute(&db).await;
        });
    }
    Ok(Some(user))
}

/// An admin, or 403 — for admin-only endpoints, so the check can't be forgotten.
pub struct AdminUser(pub User);

#[async_trait]
impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        require_role(&user, &["admin"])?;
        Ok(AdminUser(user))
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
/// It looks the caller up once (see [`resolve_user`]) and puts the `User` on
/// the request, where `AuthUser` picks it up — handlers never look it up again.
/// Upload requests take a turn for this lookup first (`admission.rs`).
pub async fn require_session(
    State(state): State<Arc<AppState>>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, ApiError> {
    // Resolve once; handlers read it from the request instead of looking it up again.
    // Uploads take turns for this one lookup, so a burst can't starve the pool.
    let gate = match crate::admission::is_upload(req.method(), req.uri().path()) {
        true => Some((crate::admission::auth_turn().await?, crate::admission::Phase::Auth.enter())),
        false => None,
    };
    let user = resolve_user(&state, req.headers()).await?.ok_or(ApiError::Unauthorized)?;
    drop(gate);
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

// =============================================================================
// Sign-in throttling: 10 failed attempts per email per minute (ratelimit.rs).
// =============================================================================

/// Frozen argon2id hash of a constant password.  Used as a side-channel
/// equaliser when the email lookup miss — running verify against this keeps
/// response time independent of whether the email exists.
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$bF93eVNDc1RmcDdLZG5pTQ$kE0n/4f6Aw0NSnVxLPq8fEPLQp9lyIULqYO0EJ3pEsM";

const LOGIN_FAILS_PER_MINUTE: u32 = 10;

// =============================================================================
// Upload rate limiting (shared across pods — see ratelimit.rs)
// =============================================================================

/// One upload by `user_id`; refuses past `UPLOAD_RATE_PER_MIN` (default 60) a minute.
pub async fn upload_rate_limit(db: &PgPool, user_id: &str) -> Result<(), ApiError> {
    crate::ratelimit::hit(db, &format!("upload:{user_id}"), crate::ratelimit::uploads_per_minute(), "Upload limit reached. Slow down.").await
}

/// Same, for callers sharing one identity (keyed by system, link token, IP…).
pub async fn upload_rate_limit_keyed(db: &PgPool, key: &str, per_minute: u32) -> Result<(), ApiError> {
    crate::ratelimit::hit(db, key, per_minute, "Too many requests. Try again in a minute.").await
}

/// Is `key` already at `per_minute`? (doesn't count a hit)
pub async fn rate_peek(db: &PgPool, key: &str, per_minute: u32) -> Result<(), ApiError> {
    crate::ratelimit::check(db, key, per_minute, "Too many requests. Try again in a minute.").await
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


// ── Self sign-up ──

#[derive(Debug, Deserialize)]
pub struct SignUp {
    pub display_name: String,
    pub email: String,
    pub password: String,
}

/// POST /api/auth/signup — anyone may ask for an account; it starts as a
/// `pending` viewer that an admin turns on (Admin → People). The answer is the
/// same whether or not the email is already registered, so the form can't be
/// used to find out who has an account. Off when workspace `self_signup` = off
/// or LOCAL_LOGIN is off.
pub async fn signup(State(s): State<Arc<AppState>>, headers: axum::http::HeaderMap, Json(b): Json<SignUp>) -> ApiResult<Json<serde_json::Value>> {
    let off: Option<String> = sqlx::query_scalar("SELECT value FROM workspace_config WHERE key = 'self_signup'").fetch_optional(&s.db).await?;
    let local_off = matches!(std::env::var("LOCAL_LOGIN").unwrap_or_default().trim(), "0" | "off" | "false" | "no");
    if off.as_deref() == Some("off") || local_off { return Err(ApiError::NotFound); }
    let ip = crate::sharelink::client_addr(&headers);
    crate::ratelimit::hit(&s.db, &format!("signup:{ip}"), 5, "Too many sign-ups from here. Try again in a minute.").await?;
    crate::ratelimit::hit(&s.db, "signup:all", 60, "Too many sign-ups right now. Try again in a minute.").await?;
    let name = b.display_name.trim();
    let email = b.email.trim().to_lowercase();
    if name.is_empty() || name.chars().count() > 80 { return Err(ApiError::BadRequest("Enter your name (up to 80 characters).".into())); }
    let at = email.find('@');
    if email.len() > 254 || at.is_none_or(|i| i == 0 || !email[i + 1..].contains('.')) || email.contains(char::is_whitespace) {
        return Err(ApiError::BadRequest("Enter a valid email address.".into()));
    }
    if b.password.chars().count() < 8 || b.password.len() > 256 { return Err(ApiError::BadRequest("The password must be at least 8 characters.".into())); }
    let pw = b.password.clone();
    let hash = tokio::task::spawn_blocking(move || hash_password(&pw)).await.map_err(|e| ApiError::Other(e.into()))??;
    let id = crate::new_id();
    let created = sqlx::query(
        "INSERT INTO users (id, email, display_name, avatar_tone, role, password_hash, status, source)
         VALUES ($1, $2, $3, 'slate', 'viewer', $4, 'pending', 'signup') ON CONFLICT DO NOTHING",
    ).bind(&id).bind(&email).bind(name).bind(&hash).execute(&s.db).await?.rows_affected();
    if created == 1 {
        let _ = sqlx::query(
            "INSERT INTO notifications (id, user_id, kind, title, body, link)
             SELECT fh_cuid(), u.id, 'signup', 'New account request', $1, '/admin?tab=people'
               FROM users u WHERE u.role = 'admin' AND u.status = 'active'",
        ).bind(format!("{name} · {email}")).execute(&s.db).await;
        tracing::info!(email = %email, "self sign-up waiting for approval");
    }
    Ok(Json(serde_json::json!({ "status": "pending" })))
}

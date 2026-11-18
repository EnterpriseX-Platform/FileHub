//! Optional signature check of the IAMX/Keycloak access token forwarded by the
//! edge. Off unless `EDGE_JWKS_URL` is set (e.g.
//! `https://sso.example/realms/neb/protocol/openid-connect/certs`).
//! `EDGE_JWT_ISSUER` (optional) must then match the token's `iss`.
//!
//! Keys are cached for an hour and refetched once when a token names an
//! unknown `kid` (key rotation).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use tokio::sync::RwLock;

struct Cache { set: Option<JwkSet>, at: Option<Instant> }

fn cache() -> &'static RwLock<Cache> {
    static C: OnceLock<RwLock<Cache>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(Cache { set: None, at: None }))
}

async fn fetch(url: &str) -> Option<JwkSet> {
    let res = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().ok()?
        .get(url).send().await.ok()?;
    if !res.status().is_success() { return None; }
    res.json::<JwkSet>().await.ok()
}

async fn keys(url: &str, force: bool) -> Option<JwkSet> {
    {
        let c = cache().read().await;
        if !force {
            if let (Some(set), Some(at)) = (&c.set, c.at) {
                if at.elapsed() < Duration::from_secs(3600) { return Some(set.clone()); }
            }
        }
    }
    let set = fetch(url).await?;
    let mut c = cache().write().await;
    c.set = Some(set.clone());
    c.at = Some(Instant::now());
    Some(set)
}

/// true when no JWKS is configured, or the token verifies.
pub async fn verify_if_configured(token: &str) -> bool {
    let Ok(url) = std::env::var("EDGE_JWKS_URL") else { return true };
    if url.trim().is_empty() { return true; }
    verify_with(&url, token).await.is_some()
}

/// Verify signature, expiry and (when `IAMX_JWT_ISSUER`/`EDGE_JWT_ISSUER` is set)
/// issuer against the keys at `url`. Returns the claims.
pub async fn verify_with(url: &str, token: &str) -> Option<serde_json::Value> {
    let header = decode_header(token).ok()?;
    let kid = header.kid.clone()?;
    let mut set = keys(url, false).await?;
    if set.find(&kid).is_none() { set = keys(url, true).await?; }
    let jwk = set.find(&kid)?;
    let key = DecodingKey::from_jwk(jwk).ok()?;
    let alg = match header.alg { Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 | Algorithm::ES256 | Algorithm::PS256 => header.alg, _ => return None };
    let mut v = Validation::new(alg);
    v.validate_aud = false; // Keycloak tokens carry many audiences; clients are limited by IAMX_ALLOWED_CLIENTS.
    v.leeway = 30;
    let iss = std::env::var("IAMX_JWT_ISSUER").ok().filter(|x| !x.trim().is_empty())
        .or_else(|| std::env::var("EDGE_JWT_ISSUER").ok().filter(|x| !x.trim().is_empty()));
    if let Some(iss) = iss { v.set_issuer(&[iss.trim()]); }
    decode::<serde_json::Value>(token, &key, &v).ok().map(|d| d.claims)
}

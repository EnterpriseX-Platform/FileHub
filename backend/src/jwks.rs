//! Optional signature check of the IAMX/Keycloak access token forwarded by the
//! edge. Off unless `EDGE_JWKS_URL` is set (e.g.
//! `https://sso.example/realms/main/protocol/openid-connect/certs`).
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

// ── Legacy issuer key (one PEM public key, not a JWKS) ──
//
// Old file services verified the Bearer token their callers send against one
// RSA public key published by the issuer (`GET <issuer>/oauth/token_key` →
// `{"kid": "...", "value": "-----BEGIN PUBLIC KEY-----..."}`). `LEGACY_JWT_KEY_URL`
// points at that URL (a raw PEM body works too); `LEGACY_JWT_PUBLIC_KEY` holds
// the PEM itself instead. Cached for an hour, refetched once when a token fails.

struct PemCache { pem: Option<String>, at: Option<Instant> }

fn pem_cache() -> &'static RwLock<PemCache> {
    static C: OnceLock<RwLock<PemCache>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(PemCache { pem: None, at: None }))
}

/// Rebuild PEM line breaks, which some issuers strip (`-----BEGIN PUBLIC KEY-----MIIB…`).
fn normalise_pem(raw: &str) -> String {
    let body: String = raw
        .replace("-----BEGIN PUBLIC KEY-----", "")
        .replace("-----END PUBLIC KEY-----", "")
        .chars().filter(|c| !c.is_whitespace()).collect();
    let lines: Vec<String> = body.as_bytes().chunks(64).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
    format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n", lines.join("\n"))
}

async fn fetch_pem(url: &str) -> Option<String> {
    let res = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().ok()?
        .get(url).send().await.ok()?;
    if !res.status().is_success() { return None; }
    let text = res.text().await.ok()?;
    let raw = serde_json::from_str::<serde_json::Value>(&text).ok()
        .and_then(|v| v.get("value").and_then(|x| x.as_str()).map(str::to_string))
        .unwrap_or(text);
    raw.contains("BEGIN PUBLIC KEY").then(|| normalise_pem(&raw))
}

async fn legacy_pem(force: bool) -> Option<String> {
    if let Some(pem) = std::env::var("LEGACY_JWT_PUBLIC_KEY").ok().filter(|v| v.contains("BEGIN PUBLIC KEY")) {
        return Some(normalise_pem(&pem));
    }
    let url = std::env::var("LEGACY_JWT_KEY_URL").ok().filter(|v| !v.trim().is_empty())?;
    if !force {
        let c = pem_cache().read().await;
        if let (Some(p), Some(at)) = (&c.pem, c.at) {
            if at.elapsed() < Duration::from_secs(3600) { return Some(p.clone()); }
        }
    }
    let pem = fetch_pem(url.trim()).await?;
    let mut c = pem_cache().write().await;
    c.pem = Some(pem.clone());
    c.at = Some(Instant::now());
    Some(pem)
}

fn decode_rsa(pem: &str, token: &str, alg: Algorithm) -> Option<serde_json::Value> {
    let key = DecodingKey::from_rsa_pem(pem.as_bytes()).ok()?;
    let mut v = Validation::new(alg);
    v.validate_aud = false;
    v.leeway = 30;
    decode::<serde_json::Value>(token, &key, &v).ok().map(|d| d.claims)
}

/// Claims of a token signed by the legacy issuer's key (signature and expiry
/// checked), or `None` when no key is configured or the token doesn't verify.
pub async fn verify_legacy(token: &str) -> Option<serde_json::Value> {
    let header = decode_header(token).ok()?;
    let alg = match header.alg { Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => header.alg, _ => return None };
    let pem = legacy_pem(false).await?;
    if let Some(c) = decode_rsa(&pem, token, alg) { return Some(c); }
    // The issuer may have rotated its key since we cached it.
    let fresh = legacy_pem(true).await?;
    if fresh == pem { return None; }
    decode_rsa(&fresh, token, alg)
}

#[cfg(test)]
mod tests {
    use super::normalise_pem;

    #[test]
    fn pem_without_line_breaks_is_rebuilt() {
        let p = normalise_pem("-----BEGIN PUBLIC KEY-----MIIBIjAN-----END PUBLIC KEY-----");
        assert_eq!(p, "-----BEGIN PUBLIC KEY-----\nMIIBIjAN\n-----END PUBLIC KEY-----\n");
    }
}

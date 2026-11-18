//! Shared helpers for HTTP integration tests.
//!
//! Tests hit a live backend (default: http://localhost:8090) so we exercise
//! the full request → DB → response pipeline — including encryption,
//! multipart uploads, and the actual axum routing. Spin the backend up with
//! `cargo run` (or docker compose) before running `cargo test`.

#![allow(dead_code)]

use reqwest::Client;
use serde_json as _;

pub const SYS_HR: &str         = "sys_hr";
pub const SYS_FIN: &str        = "sys_fin";
pub const SYS_LEGAL: &str      = "sys_legal";
pub const SYS_PROC: &str       = "sys_proc";
pub const ORG_PHATTANA: &str   = "org_phattana";
pub const ORG_HR_CENTRAL: &str = "org_hr_central";
pub const ORG_FIN_AP: &str     = "org_fin_ap";
pub const FILE_001: &str       = "00000000-0000-7000-8000-000000002001";  // contract-A12.pdf
pub const FILE_007: &str       = "00000000-0000-7000-8000-000000002007";  // budget-q1-2026.xlsx
pub const FILE_011: &str       = "00000000-0000-7000-8000-00000000200b";  // team-photo.jpg
pub const MISSING_UUID: &str   = "00000000-0000-4000-8000-fffffffffff0";

/// Base URL for the backend, including the `/fh` prefix the router mounts
/// every route under.  Override with `FILEHUB_TEST_BASE=http://staging:8090`
/// (no `/fh` — the helper appends it) to point integration tests elsewhere.
pub fn base() -> String {
    let root = std::env::var("FILEHUB_TEST_BASE").unwrap_or_else(|_| "http://localhost:8090".into());
    format!("{}/fh", root.trim_end_matches('/'))
}

/// Each tokio::test runs in its own runtime; sharing a `Client` across them
/// triggers "user code panicked / runtime dropped the dispatch task" because
/// hyper's connection pool is bound to whichever runtime created it. Building
/// per call keeps every test self-contained.
pub fn client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .cookie_store(true)
        .build()
        .expect("reqwest client")
}

/// Returns a client whose cookie jar is pre-populated with a valid session for
/// the editor account. Tests that exercise mutating endpoints should use this.
pub async fn auth_client() -> Client {
    auth_client_as("anong@acme.go.th", "anong123").await
}

/// Same as `auth_client` but lets the caller pick which seed account to log
/// in as (admin / editor / viewer).
pub async fn auth_client_as(email: &str, password: &str) -> Client {
    let c = client();
    let r = c
        .post(format!("{}/api/auth/login", base()))
        .json(&serde_json::json!({"email": email, "password": password}))
        .send()
        .await
        .expect("login send");
    assert!(r.status().is_success(), "login failed for {email}: {}", r.status());
    c
}

/// Best-effort check that a backend is reachable; if not, tests should
/// be skipped with a clear message rather than producing confusing
/// connect errors per assertion.
pub async fn require_backend() {
    let url = format!("{}/api/health", base());
    let ok = client()
        .get(&url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    if !ok {
        panic!(
            "backend not reachable at {url} — start it with `cargo run --bin filehub-backend` \
             (or set FILEHUB_TEST_BASE to a running instance)"
        );
    }
}

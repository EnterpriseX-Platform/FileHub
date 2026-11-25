//! Admission control for the write phase of uploads.
//!
//! An upload's database work is ~20 short statements. When thousands arrive at
//! once, every statement queued for a pooled connection separately, so no
//! upload could finish before the pool's acquire timeout and all of them failed.
//! Here an upload waits for a slot *before* its database phase and then runs
//! it straight through. Bytes are already staged on disk while waiting, so the
//! wait costs no database connection. Past `UPLOAD_QUEUE_WAIT_SECS` the caller
//! gets 503 + Retry-After instead of a 500.
//!
//! Two gates, neither held while a body is being received — a slow or stalled
//! client (mobile network, closed tab) must only slow its own upload, never
//! hold a turn that others need:
//! - [`auth_turn`]: the sign-in lookup of an upload request (one short query),
//!   `UPLOAD_REQUESTS` at a time per pod. A burst can't exhaust the pool on
//!   session checks alone.
//! - [`upload_slot`]: the database phase of an upload, once its bytes are
//!   staged on disk; `UPLOAD_CONCURRENCY` at a time.

use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::{Semaphore, SemaphorePermit};

use crate::config::env_num;
use crate::error::ApiError;

/// A gate: a semaphore and how many turns it has (kept for the metrics).
struct Gate { sem: Semaphore, size: usize }

impl Gate {
    fn new(key: &str) -> Gate {
        // Default: one turn per pooled database connection.
        let size = env_num(key, env_num("DATABASE_MAX_CONNECTIONS", 32)) as usize;
        Gate { sem: Semaphore::new(size), size }
    }
    fn in_use(&self) -> usize { self.size - self.sem.available_permits() }

    /// Wait for a turn, up to `UPLOAD_QUEUE_WAIT_SECS`; then 503 + Retry-After.
    async fn turn(&'static self) -> Result<SemaphorePermit<'static>, ApiError> {
        let wait = Duration::from_secs(env_num("UPLOAD_QUEUE_WAIT_SECS", 300));
        match tokio::time::timeout(wait, self.sem.acquire()).await {
            Ok(Ok(permit)) => Ok(permit),
            _ => Err(ApiError::Busy("FileHub is very busy. Try again in a moment.".into())),
        }
    }
}

fn write_gate() -> &'static Gate {
    static G: OnceLock<Gate> = OnceLock::new();
    G.get_or_init(|| Gate::new("UPLOAD_CONCURRENCY"))
}

fn auth_gate() -> &'static Gate {
    static G: OnceLock<Gate> = OnceLock::new();
    G.get_or_init(|| Gate::new("UPLOAD_REQUESTS"))
}

static WAITING: AtomicUsize = AtomicUsize::new(0);

/// A turn for an upload's database phase; hold it until the phase is done.
pub async fn upload_slot() -> Result<SemaphorePermit<'static>, ApiError> {
    write_gate().turn().await
}

/// A turn for an upload request's sign-in lookup. Drop it before reading the
/// body: a slow client must not hold a turn others need.
pub async fn auth_turn() -> Result<SemaphorePermit<'static>, ApiError> {
    WAITING.fetch_add(1, Relaxed);
    let turn = auth_gate().turn().await;
    WAITING.fetch_sub(1, Relaxed);
    turn
}

/// (upload requests waiting for a sign-in turn, uploads in their database phase).
pub fn queue_depth() -> (usize, usize) {
    (WAITING.load(Relaxed), write_gate().in_use())
}

/// Requests that send a file: web uploads, chunk uploads, upload links,
/// the legacy FileService and new versions.
pub(crate) fn is_upload(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;
    let p = path.trim_start_matches("/fh");
    match *method {
        Method::POST => p == "/api/files" || p == "/api/files/batch" || p.starts_with("/api/uploads")
            || p.starts_with("/api/upload/") || p.starts_with("/FileService/upload")
            || (p.starts_with("/api/files/") && p.ends_with("/versions")),
        Method::PATCH => p.starts_with("/api/uploads/") || p.starts_with("/api/upload/"),
        _ => false,
    }
}

/// Where admitted uploads are right now — gauges for `/fh/metrics`, so a
/// stall shows which step it is in.
#[derive(Clone, Copy)]
pub enum Phase { Auth, ReadingBody, Checks, Storing }

impl Phase {
    pub const ALL: [Phase; 4] = [Phase::Auth, Phase::ReadingBody, Phase::Checks, Phase::Storing];
    pub fn name(self) -> &'static str {
        match self { Phase::Auth => "auth", Phase::ReadingBody => "reading_body", Phase::Checks => "checks", Phase::Storing => "storing" }
    }
    /// Counts the current request in this step until the guard is dropped.
    pub fn enter(self) -> PhaseGuard { GAUGES[self as usize].fetch_add(1, Relaxed); PhaseGuard(self) }
    pub fn count(self) -> i64 { GAUGES[self as usize].load(Relaxed) }
}

static GAUGES: [AtomicI64; 4] = [AtomicI64::new(0), AtomicI64::new(0), AtomicI64::new(0), AtomicI64::new(0)];

pub struct PhaseGuard(Phase);
impl Drop for PhaseGuard { fn drop(&mut self) { GAUGES[self.0 as usize].fetch_sub(1, Relaxed); } }

#[cfg(test)]
mod tests {
    use super::is_upload;
    use axum::http::Method;

    #[test]
    fn recognises_upload_requests() {
        assert!(is_upload(&Method::POST, "/fh/api/files"));
        assert!(is_upload(&Method::PATCH, "/fh/api/uploads/0190"));
        assert!(is_upload(&Method::POST, "/FileService/upload"));
        assert!(is_upload(&Method::POST, "/fh/api/files/abc/versions"));
        assert!(!is_upload(&Method::GET, "/fh/api/files"));
        assert!(!is_upload(&Method::POST, "/fh/api/files/abc/status"));
    }
}

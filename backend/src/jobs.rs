//! Bounded background work, so a burst can never exhaust memory or processes.
//!
//! - Indexing after upload (text, thumbnail, Office preview): at most
//!   `P1_CONCURRENCY` at once (default: CPU count). When `P1_QUEUE_MAX` uploads
//!   (default 2000) are already waiting, new ones are skipped — the file is
//!   stored and usable; its thumbnail/preview is made on first view instead.
//! - LibreOffice conversions: at most `OFFICE_CONCURRENCY` at once (default 2;
//!   each process takes ~200 MB) and `OFFICE_TIMEOUT_SECS` each (default 120),
//!   after which the process is killed.

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::OnceLock;

use tokio::sync::Semaphore;

use crate::config::env_num;

fn cpus() -> u64 { std::thread::available_parallelism().map(|n| n.get() as u64).unwrap_or(2) }

fn index_slots() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| Semaphore::new(env_num("P1_CONCURRENCY", cpus()) as usize))
}

fn office_slots() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| Semaphore::new(env_num("OFFICE_CONCURRENCY", 2) as usize))
}

static QUEUED: AtomicUsize = AtomicUsize::new(0);

/// Run `job` in the background with a bounded number at once; dropped (not
/// queued) when the backlog is already full.
pub fn spawn_indexing<F>(job: F)
where F: std::future::Future<Output = ()> + Send + 'static {
    if QUEUED.load(Relaxed) >= env_num("P1_QUEUE_MAX", 2000) as usize {
        tracing::warn!("indexing backlog full — skipping; preview/thumbnail will be made on first view");
        return;
    }
    QUEUED.fetch_add(1, Relaxed);
    tokio::spawn(async move {
        let permit = index_slots().acquire().await;
        QUEUED.fetch_sub(1, Relaxed);
        if permit.is_ok() { job.await; }
    });
}

/// Uploads waiting to be indexed (for metrics).
pub fn indexing_backlog() -> usize { QUEUED.load(Relaxed) }

/// Office file → PDF with LibreOffice, a few at a time, each time-limited.
pub async fn office_to_pdf(file_name: String, body: Vec<u8>) -> Option<Vec<u8>> {
    let _turn = office_slots().acquire().await.ok()?;
    let timeout = std::time::Duration::from_secs(env_num("OFFICE_TIMEOUT_SECS", 120));
    tokio::task::spawn_blocking(move || crate::p1::convert_to_pdf(&file_name, &body, timeout)).await.ok().flatten()
}

/// Refuse work that would hold a whole file in memory when it's too big
/// (`INMEMORY_MAX_MB`, default 200): stamping, on-demand conversion.
pub fn fits_in_memory(size: i64, what: &str) -> Result<(), crate::error::ApiError> {
    let max = env_num("INMEMORY_MAX_MB", 200) * 1024 * 1024;
    if size as u64 > max {
        return Err(crate::error::ApiError::PayloadTooLarge(format!("This file is too large to {what} ({} MB at most).", max / 1024 / 1024)));
    }
    Ok(())
}


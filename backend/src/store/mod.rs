//! Pluggable object-storage abstraction.
//!
//! Two backends ship in-tree:
//!   * [`fs`]: posix filesystem under `STORAGE_ROOT`. Default.
//!   * [`s3`]: S3 (and S3-compatible — MinIO, Wasabi, R2) via direct HTTP +
//!     hand-rolled SigV4. No `aws-sdk-s3` dependency.
//!
//! Switch with `STORAGE_BACKEND=fs|s3`. Encryption (AES-256-GCM, see the
//! `Storage` wrapper in `storage.rs`) runs in front of whichever backend is
//! configured.

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;

pub mod fs;
pub mod s3;

/// What every storage backend agrees on. `key` is a path-style identifier
/// like `hr-emp-files/file-<uuid>-name.ext` — backends are free to interpret
/// the prefix as a bucket / prefix / folder.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Write the bytes. Returns when the object is durable.  Overwrites any
    /// existing object at the same key.
    async fn put(&self, key: &str, body: Bytes) -> Result<()>;

    /// Read the entire object.  Returns `Ok(None)` for not-found so callers
    /// don't need to inspect error chains.
    async fn get(&self, key: &str) -> Result<Option<Bytes>>;

    /// Remove the object.  Missing → success (idempotent).
    async fn delete(&self, key: &str) -> Result<()>;

    /// Human-readable backend label for log lines and the dashboard's
    /// "Source" badge.
    fn label(&self) -> &str;
}

/// Build the configured backend from env. Falls back to filesystem.
pub async fn from_env() -> Result<std::sync::Arc<dyn ObjectStore>> {
    let backend = std::env::var("STORAGE_BACKEND").unwrap_or_else(|_| "fs".into());
    match backend.as_str() {
        "s3" => Ok(std::sync::Arc::new(s3::S3Store::from_env()?)),
        _    => {
            let root = std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./storage".into());
            Ok(std::sync::Arc::new(fs::FsStore::init(&root).await?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper used by both fs and s3 test suites: every backend must satisfy
    /// these invariants. Backends call this from their own `#[cfg(test)]`
    /// modules so the contract stays uniform.
    pub async fn contract_round_trip(store: &dyn ObjectStore) {
        let key = "bucket-1/key-with/slashes.bin";
        let payload = Bytes::from_static(b"\x00\xff\x10contract-bytes");
        store.put(key, payload.clone()).await.unwrap();
        let got = store.get(key).await.unwrap().expect("just wrote it");
        assert_eq!(&got[..], &payload[..]);
        store.delete(key).await.unwrap();
        assert!(store.get(key).await.unwrap().is_none());
        // delete is idempotent
        store.delete(key).await.unwrap();
    }

    #[allow(dead_code)]
    pub(super) async fn _expose() {
        // No-op — only exists so cargo doesn't warn when only one backend's
        // tests use `contract_round_trip` in a given build.
        let _ = contract_round_trip;
    }
}

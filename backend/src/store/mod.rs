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

use std::path::Path;
use std::pin::Pin;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;

/// Stream of raw object bytes — what [`ObjectStore::open_range`] hands back so
/// large objects never have to sit in memory in one piece.
pub type ByteStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + 'static>>;

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

    /// Size of the stored object in bytes, `Ok(None)` when missing.
    /// Default reads the whole object — backends should override.
    async fn size(&self, key: &str) -> Result<Option<u64>> {
        Ok(self.get(key).await?.map(|b| b.len() as u64))
    }

    /// Stream `len` bytes starting at `offset` (clamped to the object end).
    /// Default reads the whole object and slices it — backends that can seek
    /// (filesystem, S3 Range GET) override this so memory stays O(chunk).
    async fn open_range(&self, key: &str, offset: u64, len: u64) -> Result<Option<ByteStream>> {
        let Some(all) = self.get(key).await? else { return Ok(None) };
        let start = (offset as usize).min(all.len());
        let end = start.saturating_add(len as usize).min(all.len());
        let part = all.slice(start..end);
        Ok(Some(Box::pin(futures::stream::once(async move { Ok(part) }))))
    }

    /// Move a finished local file (written by the staging step) into the
    /// store under `key`.  The source file is consumed.  Default reads it
    /// into memory and calls [`put`](Self::put); the filesystem backend
    /// renames it instead so no byte is copied.
    async fn put_path(&self, key: &str, src: &Path) -> Result<()> {
        let body = tokio::fs::read(src).await?;
        self.put(key, Bytes::from(body)).await?;
        let _ = tokio::fs::remove_file(src).await;
        Ok(())
    }

    /// Move an object to a new key.  Default copies through memory; the
    /// filesystem backend renames.
    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        if let Some(b) = self.get(from).await? {
            self.put(to, b).await?;
            self.delete(from).await?;
        }
        Ok(())
    }

    /// Directory where staged uploads are written before [`put_path`].
    /// Same filesystem as the store when possible so the final move is a rename.
    fn staging_dir(&self) -> std::path::PathBuf {
        std::env::temp_dir().join("filehub-staging")
    }
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

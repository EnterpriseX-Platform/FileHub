//! AES-256-GCM encryption layer over an [`ObjectStore`].  Keeps the
//! application code unaware of whether bytes land on the local filesystem,
//! a MinIO bucket, or anywhere else the trait can describe.

use std::sync::Arc;

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};
use bytes::Bytes;
use md5::{Digest, Md5};

use crate::store::{self, ObjectStore};

pub struct Storage {
    backend: Arc<dyn ObjectStore>,
    cipher:  Option<Aes256Gcm>,
}

impl Storage {
    /// Wrap an arbitrary [`ObjectStore`] with optional AES-256-GCM
    /// encryption.  Production code calls [`Storage::init`] which builds the
    /// backend from env; tests pass concrete backends in via this entry
    /// point so they can stay process-isolated.
    pub fn new(backend: Arc<dyn ObjectStore>, key: Option<[u8; 32]>) -> Self {
        let cipher = key.map(|bytes| {
            let k = Key::<Aes256Gcm>::from_slice(&bytes);
            tracing::info!("storage encryption: enabled (AES-256-GCM)");
            Aes256Gcm::new(k)
        });
        if cipher.is_none() {
            tracing::info!("storage encryption: disabled (no key)");
        }
        Self { backend, cipher }
    }

    /// Production entry point — picks the backend from `STORAGE_BACKEND` and
    /// the cipher key from `STORAGE_ENC_KEY`.
    pub async fn init(_root_unused: impl AsRef<std::path::Path>) -> Result<Self> {
        let backend = store::from_env().await?;
        tracing::info!("storage backend = {}", backend.label());

        let key = match std::env::var("STORAGE_ENC_KEY") {
            Ok(hex_key) => {
                let bytes = hex::decode(hex_key.trim())
                    .context("STORAGE_ENC_KEY must be hex")?;
                if bytes.len() != 32 {
                    anyhow::bail!("STORAGE_ENC_KEY must decode to 32 bytes (got {})", bytes.len());
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(arr)
            }
            Err(_) => None,
        };

        Ok(Self::new(backend, key))
    }

    pub fn encryption_enabled(&self) -> bool {
        self.cipher.is_some()
    }

    /// Human-readable backend label (`"filesystem(/abs/path)"`, `"s3(bucket@endpoint)"`).
    /// Used by the settings page to show the actual storage destination
    /// instead of a hard-coded "Local filesystem" string.
    pub fn backend_label(&self) -> String {
        self.backend.label().to_string()
    }

    /// Write the object, returning an MD5 of the **plaintext** so the ETag
    /// stays stable across re-keying.
    pub async fn put(&self, key: &str, body: Bytes, _content_type: Option<&str>) -> Result<String> {
        let mut h = Md5::new();
        h.update(&body);
        let etag = hex::encode(h.finalize());

        let on_disk = match &self.cipher {
            Some(cipher) => {
                let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
                let ct = cipher
                    .encrypt(&nonce, body.as_ref())
                    .map_err(|e| anyhow::anyhow!("aes-gcm encrypt: {e}"))?;
                let mut out = Vec::with_capacity(12 + ct.len());
                out.extend_from_slice(nonce.as_slice());
                out.extend_from_slice(&ct);
                Bytes::from(out)
            }
            None => body,
        };

        self.backend.put(key, on_disk).await?;
        Ok(etag)
    }

    pub async fn get(&self, key: &str, encrypted: bool) -> Result<Option<(Bytes, Option<String>)>> {
        let raw = match self.backend.get(key).await? {
            Some(b) => b,
            None    => return Ok(None),
        };

        let plaintext = if encrypted {
            let cipher = self.cipher.as_ref()
                .context("file is flagged encrypted but STORAGE_ENC_KEY is not configured")?;
            if raw.len() < 12 {
                anyhow::bail!("encrypted blob too short");
            }
            let (nonce_bytes, ct) = raw.split_at(12);
            let nonce = Nonce::from_slice(nonce_bytes);
            cipher
                .decrypt(nonce, ct)
                .map_err(|e| anyhow::anyhow!("aes-gcm decrypt: {e}"))?
        } else {
            raw.to_vec()
        };

        let ct = mime_guess::from_path(key).first().map(|m| m.to_string());
        Ok(Some((Bytes::from(plaintext), ct)))
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        self.backend.delete(key).await
    }
}

// =============================================================================
// Unit tests — encrypt/decrypt invariants across whichever backend.
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::fs::FsStore;

    fn key_32() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(31);
        }
        k
    }

    async fn make(tag: &str, enc: bool) -> Storage {
        let pid = std::process::id();
        let ts  = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("filehub-storage-{pid}-{ts}-{tag}"));
        let backend = Arc::new(FsStore::init(&dir).await.unwrap());
        Storage::new(backend, if enc { Some(key_32()) } else { None })
    }

    #[tokio::test]
    async fn round_trip_plaintext() {
        let s = make("plain", false).await;
        let payload = Bytes::from_static(b"hello world");
        let etag = s.put("bucket/key.txt", payload.clone(), None).await.unwrap();
        assert_eq!(etag.len(), 32);
        let (got, _) = s.get("bucket/key.txt", false).await.unwrap().unwrap();
        assert_eq!(&got[..], &payload[..]);
    }

    #[tokio::test]
    async fn round_trip_encrypted() {
        let s = make("enc", true).await;
        let marker = "SECRET_MARKER_XYZ";
        s.put("b/k.txt", Bytes::from(marker.to_string()), None).await.unwrap();
        // Read raw bytes from backend — confirm plaintext does not appear.
        let raw = s.backend.get("b/k.txt").await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains(marker));
        // Decrypted read returns the original bytes.
        let (decrypted, _) = s.get("b/k.txt", true).await.unwrap().unwrap();
        assert_eq!(&decrypted[..], marker.as_bytes());
    }

    #[tokio::test]
    async fn get_missing_is_none() {
        let s = make("missing", false).await;
        assert!(s.get("nope/abc", false).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_missing_idempotent() {
        let s = make("del-idem", false).await;
        s.delete("nope/abc").await.unwrap();
        s.delete("nope/abc").await.unwrap();
    }

    #[tokio::test]
    async fn encryption_disabled_when_no_key() {
        let s = make("noenc", false).await;
        assert!(!s.encryption_enabled());
    }
}

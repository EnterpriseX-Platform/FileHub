//! Filesystem ObjectStore — the historical implementation.  Stores blobs
//! under `<root>/<key>` with directory traversal explicitly blocked.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::ObjectStore;

pub struct FsStore {
    pub root: PathBuf,
}

impl FsStore {
    pub async fn init(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .await
            .with_context(|| format!("creating storage root {}", root.display()))?;
        Ok(Self { root })
    }

    fn path_for(&self, key: &str) -> Result<PathBuf> {
        for seg in key.split('/') {
            if seg == ".." {
                anyhow::bail!("invalid object key (contains ..): {key}");
            }
        }
        Ok(self.root.join(key))
    }
}

#[async_trait]
impl ObjectStore for FsStore {
    async fn put(&self, key: &str, body: Bytes) -> Result<()> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await
                .with_context(|| format!("creating parent dir for {}", path.display()))?;
        }
        // Write-then-rename: a half-written `<key>` is worse than no key at
        // all, because the next reader returns truncated bytes with no error
        // signal.  We stream into `<key>.tmp.<rand>`, fsync the file so the
        // bytes hit disk, atomically rename over the destination, then fsync
        // the parent directory so the rename itself survives a power loss.
        // The random suffix keeps two concurrent writes to the same key from
        // clobbering each other's temp files.
        let suffix: u64 = rand::random::<u64>();
        let tmp_name = format!("{}.tmp.{suffix}",
            path.file_name().and_then(|s| s.to_str()).unwrap_or("blob"));
        let tmp_path = path.with_file_name(tmp_name);
        {
            let mut f = fs::File::create(&tmp_path).await
                .with_context(|| format!("creating tmp {}", tmp_path.display()))?;
            f.write_all(&body).await?;
            f.flush().await?;
            f.sync_all().await
                .with_context(|| format!("fsync {}", tmp_path.display()))?;
        }
        fs::rename(&tmp_path, &path).await
            .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()))?;
        // fsync the directory so the rename hits disk too.  Best-effort —
        // some filesystems (tmpfs, virtio-9p) reject open(dir) and that
        // shouldn't take the upload down.
        if let Some(parent) = path.parent() {
            if let Ok(f) = fs::File::open(parent).await {
                let _ = f.sync_all().await;
            }
        }
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Bytes>> {
        let path = self.path_for(key)?;
        match fs::read(&path).await {
            Ok(d) => Ok(Some(Bytes::from(d))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.path_for(key)?;
        match fs::remove_file(&path).await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("deleting {}", path.display())),
        }
    }

    fn label(&self) -> &str { "filesystem" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let ts  = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("filehub-store-{pid}-{ts}-{tag}"))
    }

    #[tokio::test]
    async fn round_trip() {
        let s = FsStore::init(temp_root("rt")).await.unwrap();
        super::super::tests::contract_round_trip(&s).await;
    }

    #[tokio::test]
    async fn path_traversal_rejected() {
        let s = FsStore::init(temp_root("trav")).await.unwrap();
        let err = s.put("../escape.txt", Bytes::from_static(b"x")).await.unwrap_err();
        assert!(err.to_string().contains(".."));
    }

    #[tokio::test]
    async fn label_reads_filesystem() {
        let s = FsStore::init(temp_root("label")).await.unwrap();
        assert_eq!(s.label(), "filesystem");
    }
}

//! Filesystem ObjectStore — the historical implementation.  Stores blobs
//! under `<root>/<key>` with directory traversal explicitly blocked.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use super::{ByteStream, ObjectStore};

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

    async fn size(&self, key: &str) -> Result<Option<u64>> {
        let path = self.path_for(key)?;
        match fs::metadata(&path).await {
            Ok(m) => Ok(Some(m.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("stat {}", path.display())),
        }
    }

    /// Seek + stream — memory per request stays at the reader buffer size no
    /// matter how large the object is (a 2 GB video costs the same as a PDF).
    async fn open_range(&self, key: &str, offset: u64, len: u64) -> Result<Option<ByteStream>> {
        let path = self.path_for(key)?;
        let mut f = match fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
        };
        if offset > 0 {
            f.seek(std::io::SeekFrom::Start(offset)).await?;
        }
        let reader = f.take(len);
        Ok(Some(Box::pin(tokio_util::io::ReaderStream::with_capacity(reader, 256 * 1024))))
    }

    /// Staged file → final key by rename (same filesystem, see `staging_dir`),
    /// with the same fsync discipline as [`put`](ObjectStore::put).  Falls back
    /// to copy when the rename crosses devices.
    async fn put_path(&self, key: &str, src: &Path) -> Result<()> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await
                .with_context(|| format!("creating parent dir for {}", path.display()))?;
        }
        if let Ok(f) = fs::OpenOptions::new().write(true).open(src).await {
            f.sync_all().await.with_context(|| format!("fsync {}", src.display()))?;
        }
        if fs::rename(src, &path).await.is_err() {
            fs::copy(src, &path).await
                .with_context(|| format!("copying {} -> {}", src.display(), path.display()))?;
            if let Ok(f) = fs::OpenOptions::new().write(true).open(&path).await {
                let _ = f.sync_all().await;
            }
            let _ = fs::remove_file(src).await;
        }
        if let Some(parent) = path.parent() {
            if let Ok(f) = fs::File::open(parent).await {
                let _ = f.sync_all().await;
            }
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let src = self.path_for(from)?;
        let dst = self.path_for(to)?;
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).await?;
        }
        match fs::rename(&src, &dst).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => {
                fs::copy(&src, &dst).await?;
                let _ = fs::remove_file(&src).await;
                Ok(())
            }
        }
    }

    fn staging_dir(&self) -> PathBuf {
        self.root.join(".staging")
    }
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

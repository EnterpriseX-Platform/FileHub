//! AES-256-GCM encryption layer over an [`ObjectStore`].  Keeps the
//! application code unaware of whether bytes land on the local filesystem,
//! a MinIO bucket, or anywhere else the trait can describe.

use std::path::PathBuf;
use std::sync::Arc;

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use md5::{Digest, Md5};
use tokio::io::AsyncWriteExt;

use crate::store::{self, ByteStream, ObjectStore};

// ---------------------------------------------------------------------------
// Encryption format v2 — chunked AES-256-GCM (STREAM construction)
//
//   header (20 B) = "FHXENC02" | chunk size u32 LE | nonce prefix (7 B) | 0
//   chunk i       = AES-GCM(plain[i*C .. (i+1)*C]) || tag (16 B)
//   nonce         = prefix (7 B) | last-chunk flag (1 B) | i as u32 BE
//   AAD           = header
//
// Each chunk authenticates on its own, so a Range read decrypts only the
// chunks it touches (memory O(chunk), not O(file)).  The index in the nonce
// stops chunk reordering; the last-chunk flag stops truncation.  Objects
// written before v2 ("v1" = 12-byte nonce + one GCM blob) are still read
// through the old whole-object path — they are told apart by the 8-byte
// magic (a v1 random nonce matches it with probability 2^-64).
// ---------------------------------------------------------------------------
pub const V2_MAGIC: &[u8; 8] = b"FHXENC02";

// ---------------------------------------------------------------------------
// Format v3 — asymmetric (folders set to "RSA"): a random 256-bit data key per
// object, wrapped with the server's RSA public key (OAEP-SHA256), followed by
// an ordinary v2 object encrypted with that data key:
//   "FHXENC03" | wrapped length u16 LE | wrapped data key | v2 object
// The RSA private key never touches the database: it comes from
// `STORAGE_RSA_PRIVATE_KEY_FILE` (a PEM file, e.g. a separately mounted
// Secret) or `STORAGE_RSA_PRIVATE_KEY`. Lose it and v3 objects can't be read.
pub const V3_MAGIC: &[u8; 8] = b"FHXENC03";
const V3_FIXED: usize = 10;

fn load_rsa_from_env() -> Option<Arc<rsa::RsaPrivateKey>> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    let pem = match std::env::var("STORAGE_RSA_PRIVATE_KEY_FILE").ok().filter(|v| !v.trim().is_empty()) {
        Some(path) => match std::fs::read_to_string(path.trim()) {
            Ok(p) => p,
            Err(e) => { tracing::warn!("storage encryption: can't read STORAGE_RSA_PRIVATE_KEY_FILE: {e}"); return None; }
        },
        None => std::env::var("STORAGE_RSA_PRIVATE_KEY").ok().filter(|v| v.contains("PRIVATE KEY"))?,
    };
    let key = rsa::RsaPrivateKey::from_pkcs8_pem(pem.trim()).ok()
        .or_else(|| rsa::RsaPrivateKey::from_pkcs1_pem(pem.trim()).ok());
    match key {
        Some(k) => { tracing::info!("storage encryption: RSA key loaded (asymmetric folders available)"); Some(Arc::new(k)) }
        None => { tracing::warn!("storage encryption: RSA private key is not a valid PEM (PKCS#8 or PKCS#1)"); None }
    }
}

/// A fresh data key and its v3 preamble (magic, length, wrapped key).
fn v3_new_key(rsa_key: &rsa::RsaPrivateKey) -> Result<(Aes256Gcm, Vec<u8>)> {
    let mut dk = [0u8; 32];
    rand::Rng::fill(&mut rand::thread_rng(), &mut dk[..]);
    let public = rsa::RsaPublicKey::from(rsa_key);
    let wrapped = public.encrypt(&mut rand::thread_rng(), rsa::Oaep::new::<sha2::Sha256>(), &dk)
        .map_err(|e| anyhow::anyhow!("rsa wrap: {e}"))?;
    let mut pre = Vec::with_capacity(V3_FIXED + wrapped.len());
    pre.extend_from_slice(V3_MAGIC);
    pre.extend_from_slice(&(wrapped.len() as u16).to_le_bytes());
    pre.extend_from_slice(&wrapped);
    Ok((Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dk)), pre))
}

fn v3_unwrap(rsa_key: Option<&rsa::RsaPrivateKey>, wrapped: &[u8]) -> Result<[u8; 32]> {
    let k = rsa_key.context("file is encrypted with the RSA key but STORAGE_RSA_PRIVATE_KEY(_FILE) is not configured")?;
    let dk = k.decrypt(rsa::Oaep::new::<sha2::Sha256>(), wrapped).map_err(|e| anyhow::anyhow!("rsa unwrap: {e}"))?;
    if dk.len() != 32 { anyhow::bail!("rsa unwrap: data key has {} bytes", dk.len()); }
    let mut a = [0u8; 32];
    a.copy_from_slice(&dk);
    Ok(a)
}
pub const V2_CHUNK: usize = 64 * 1024;
const V2_TAG: usize = 16;
pub const V2_HEADER: usize = 20;

fn v2_header(prefix: &[u8; 7]) -> [u8; V2_HEADER] {
    let mut h = [0u8; V2_HEADER];
    h[..8].copy_from_slice(V2_MAGIC);
    h[8..12].copy_from_slice(&(V2_CHUNK as u32).to_le_bytes());
    h[12..19].copy_from_slice(prefix);
    h
}

fn v2_nonce(prefix: &[u8; 7], idx: u32, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..7].copy_from_slice(prefix);
    n[7] = last as u8;
    n[8..].copy_from_slice(&idx.to_be_bytes());
    n
}

/// How an object is laid out on the backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Plain,
    /// Legacy single-shot AES-GCM (12-byte nonce + ciphertext + tag).
    V1,
    /// Chunked AES-GCM, see module docs.
    V2 { chunk: usize, chunks: u64, prefix: [u8; 7], header: [u8; V2_HEADER] },
    /// v3: an RSA-wrapped data key, then a v2 object starting at `base`.
    V3 { chunk: usize, chunks: u64, prefix: [u8; 7], header: [u8; V2_HEADER], base: u64, dk: [u8; 32] },
}

/// Result of [`Storage::stat`] — enough to answer a Range request without
/// reading the body.
#[derive(Clone, Debug)]
pub struct ObjInfo {
    /// Plaintext size in bytes (what the client sees).
    pub size: u64,
    pub stored: u64,
    pub layout: Layout,
}

/// Two-character subfolder for a new object or staged file, from the end of
/// its (random) id. On NFS, files created in one directory are handled one at
/// a time by the server; spreading them over subfolders roughly doubles
/// parallel upload throughput. Existing keys are stored in the database and
/// keep working as they are.
pub fn shard(id: &str) -> String {
    let s: String = id.chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
    if s.len() == 2 && s.chars().all(|c| c.is_ascii_alphanumeric()) { s.to_ascii_lowercase() } else { "00".into() }
}

/// An upload written to the staging area but not yet visible under its key.
pub struct Staged {
    pub path: PathBuf,
    pub size: u64,
    pub etag: String,
    /// Whether the staged bytes are already encrypted (v2 chunked).
    pub encrypted: bool,
    /// First bytes of the plaintext, for the type check ([`crate::scan`]).
    pub head: Vec<u8>,
    /// Malware scan result.
    pub verdict: crate::scan::Verdict,
}

impl Staged {
    /// FileHub's own checks on the file: real type matches the name, not a
    /// program, no malware. `allowed` = the basket's allowed extensions.
    pub fn vet(&self, name: &str, allowed: &[String]) -> std::result::Result<(), String> {
        crate::scan::check_type(name, &self.head, allowed)?;
        match crate::scan::refusal(name, &self.verdict) { Some(m) => Err(m), None => Ok(()) }
    }
}

/// Incremental writer for [`Storage::begin_stage`] — encrypts chunk by chunk
/// and hashes as bytes arrive, so an upload of any size costs O(chunk) RAM.
pub struct StageWriter {
    file:   tokio::io::BufWriter<tokio::fs::File>,
    path:   PathBuf,
    cipher: Option<Aes256Gcm>,
    prefix: [u8; 7],
    header: [u8; V2_HEADER],
    idx:    u32,
    buf:    BytesMut,
    md5:    Md5,
    size:   u64,
    head:   Vec<u8>,
    clam:   Option<crate::scan::ClamStream>,
}

impl StageWriter {
    /// Plaintext bytes written so far.
    pub fn size(&self) -> u64 { self.size }

    pub async fn write(&mut self, data: &[u8]) -> Result<()> {
        if self.head.len() < crate::scan::HEAD {
            let n = (crate::scan::HEAD - self.head.len()).min(data.len());
            self.head.extend_from_slice(&data[..n]);
        }
        if let Some(c) = self.clam.as_mut() { c.write(data).await; }
        self.md5.update(data);
        self.size += data.len() as u64;
        if self.cipher.is_none() {
            self.file.write_all(data).await?;
            return Ok(());
        }
        self.buf.extend_from_slice(data);
        // Keep the tail chunk pending: only when the stream ends do we know
        // which chunk is last (its nonce carries the last-chunk flag).
        while self.buf.len() > V2_CHUNK {
            let chunk = self.buf.split_to(V2_CHUNK);
            self.emit(&chunk, false).await?;
        }
        Ok(())
    }

    async fn emit(&mut self, plain: &[u8], last: bool) -> Result<()> {
        let cipher = self.cipher.as_ref().expect("emit only with cipher");
        let nonce = v2_nonce(&self.prefix, self.idx, last);
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: &self.header })
            .map_err(|e| anyhow::anyhow!("aes-gcm encrypt: {e}"))?;
        self.file.write_all(&ct).await?;
        self.idx = self.idx.checked_add(1).context("object too large for v2 chunk index")?;
        Ok(())
    }

    pub async fn finish(mut self) -> Result<Staged> {
        if self.cipher.is_some() {
            let rest = self.buf.split();
            self.emit(&rest, true).await?;
        }
        self.file.flush().await?;
        let encrypted = self.cipher.is_some();
        let verdict = match self.clam.take() { Some(c) => c.finish().await, None => crate::scan::Verdict::Skipped };
        Ok(Staged { path: self.path, size: self.size, etag: hex::encode(self.md5.finalize()), encrypted, head: self.head, verdict })
    }

    pub async fn abort(self) {
        drop(self.file);
        let _ = tokio::fs::remove_file(&self.path).await;
    }
}

#[derive(Clone)]
pub struct Storage {
    backend: Arc<dyn ObjectStore>,
    /// Folders mounted into baskets (`mount:/abs/path` keys), kept as plain files.
    mounts: Arc<store::fs::FsStore>,
    cipher:  Option<Aes256Gcm>,
    /// RSA key for asymmetric folders (format v3); kept out of the database.
    rsa: Option<Arc<rsa::RsaPrivateKey>>,
    /// STORAGE_ENCRYPT_ALL (default true): with a key, encrypt every object.
    /// false = encrypt only where a basket/folder policy asks for it.
    encrypt_all: bool,
}

impl Storage {
    /// Wrap an arbitrary [`ObjectStore`] with optional AES-256-GCM
    /// encryption.  Production code calls [`Storage::init`] which builds the
    /// backend from env; tests pass concrete backends in via this entry
    /// point so they can stay process-isolated.
    pub fn new(backend: Arc<dyn ObjectStore>, key: Option<[u8; 32]>) -> Self {
        let cipher = key.map(|bytes| {
            let k = Key::<Aes256Gcm>::from_slice(&bytes);
            tracing::info!("storage encryption: enabled (AES-256-GCM, chunked v2 for new objects)");
            Aes256Gcm::new(k)
        });
        if cipher.is_none() {
            tracing::info!("storage encryption: disabled (no key)");
        }
        let encrypt_all = !matches!(std::env::var("STORAGE_ENCRYPT_ALL").ok().as_deref(), Some("false") | Some("0") | Some("off"));
        if cipher.is_some() && !encrypt_all {
            tracing::info!("storage encryption: per basket/folder policy (STORAGE_ENCRYPT_ALL=false)");
        }
        let mounts = Arc::new(store::fs::FsStore { root: std::path::PathBuf::from("/") });
        Self { backend, mounts, cipher, rsa: load_rsa_from_env(), encrypt_all }
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

    /// New objects are encrypted unless a policy says otherwise.
    pub fn encryption_enabled(&self) -> bool {
        self.cipher.is_some() && self.encrypt_all
    }

    /// A key is configured, so objects *can* be encrypted.
    pub fn has_key(&self) -> bool {
        self.cipher.is_some()
    }

    /// An RSA key is configured, so folders can use asymmetric encryption.
    pub fn has_rsa_key(&self) -> bool {
        self.rsa.is_some()
    }

    /// Human-readable backend label (`"filesystem(/abs/path)"`, `"s3(bucket@endpoint)"`).
    /// Used by the settings page to show the actual storage destination
    /// instead of a hard-coded "Local filesystem" string.
    pub fn backend_label(&self) -> String {
        self.backend.label().to_string()
    }

    // ---- streaming write -------------------------------------------------

    /// Start a staged upload.  Feed it with [`StageWriter::write`], then
    /// [`StageWriter::finish`] and [`Storage::commit`] under the final key
    /// (after quota checks, which need the size).
    pub async fn begin_stage(&self) -> Result<StageWriter> {
        let dir = self.backend.staging_dir();
        tokio::fs::create_dir_all(&dir).await
            .with_context(|| format!("creating staging dir {}", dir.display()))?;
        let sid = crate::Id::new().to_string();
        let dir = dir.join(shard(&sid));
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("up-{sid}"));
        let f = tokio::fs::File::create(&path).await
            .with_context(|| format!("creating staged file {}", path.display()))?;
        let mut prefix = [0u8; 7];
        rand::Rng::fill(&mut rand::thread_rng(), &mut prefix[..]);
        let header = v2_header(&prefix);
        let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, f);
        let cipher = if self.encryption_enabled() { self.cipher.clone() } else { None };
        if cipher.is_some() {
            file.write_all(&header).await?;
        }
        Ok(StageWriter {
            file, path, cipher, prefix, header,
            idx: 0, buf: BytesMut::with_capacity(V2_CHUNK * 2), md5: Md5::new(), size: 0,
            head: Vec::with_capacity(crate::scan::HEAD), clam: crate::scan::ClamStream::open().await,
        })
    }

    /// Stage a byte stream end-to-end (convenience for callers that already
    /// have a `Stream`, e.g. a TUS temp file or an upstream response).
    pub async fn stage_stream<S, E>(&self, mut stream: S) -> Result<Staged>
    where
        S: futures::Stream<Item = std::result::Result<Bytes, E>> + Unpin,
        E: std::fmt::Display,
    {
        let mut w = self.begin_stage().await?;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(c) => if let Err(e) = w.write(&c).await { w.abort().await; return Err(e) },
                Err(e) => { w.abort().await; anyhow::bail!("reading upload stream: {e}") }
            }
        }
        w.finish().await
    }

    /// Remove staged uploads left behind by a crash or an aborted request
    /// (older than `max_age`).  Called once at startup.
    pub async fn sweep_staging(&self, max_age: std::time::Duration) -> usize {
        let dir = self.backend.staging_dir();
        let Ok(mut rd) = tokio::fs::read_dir(&dir).await else { return 0 };
        let mut n = 0;
        // Staged files sit directly in the staging dir (older pods) or one level down in a shard.
        let mut entries = Vec::new();
        while let Ok(Some(e)) = rd.next_entry().await {
            if e.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                if let Ok(mut sub) = tokio::fs::read_dir(e.path()).await {
                    while let Ok(Some(x)) = sub.next_entry().await { entries.push(x); }
                }
            } else { entries.push(e); }
        }
        for e in entries {
            let old = e.metadata().await.ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|age| age > max_age)
                .unwrap_or(false);
            if old && tokio::fs::remove_file(e.path()).await.is_ok() { n += 1; }
        }
        n
    }

    /// The store that holds `key`: a mounted folder (checked against
    /// MOUNT_ROOTS) or the main backend.
    fn be(&self, key: &str) -> Result<&dyn ObjectStore> {
        if is_mount(key) {
            crate::mount::check_path(&key[MOUNT.len()..])?;
            Ok(self.mounts.as_ref())
        } else {
            Ok(self.backend.as_ref())
        }
    }
    /// The key as the chosen store sees it (an absolute path made relative to "/").
    fn k(key: &str) -> &str {
        if is_mount(key) { key[MOUNT.len()..].trim_start_matches('/') } else { key }
    }

    /// Make a staged upload visible under `key`.  Returns the plaintext MD5.
    pub async fn commit(&self, staged: Staged, key: &str) -> Result<String> {
        self.be(key)?.put_path(Self::k(key), &staged.path).await?;
        Ok(staged.etag)
    }

    /// Commit with an explicit encryption choice. A plaintext staging file that
    /// must be encrypted is re-staged through the cipher first. Encrypted bytes
    /// are never decrypted to satisfy "no encryption". Returns (etag, encrypted).
    pub async fn commit_as(&self, staged: Staged, key: &str, encrypt: bool) -> Result<(String, bool)> {
        self.commit_mode(staged, key, encrypt, false).await
    }

    /// [`Storage::commit_as`] with a choice of asymmetric (RSA, format v3)
    /// encryption, for folders whose policy asks for it.
    pub async fn commit_mode(&self, staged: Staged, key: &str, encrypt: bool, asymmetric: bool) -> Result<(String, bool)> {
        if encrypt && asymmetric && !is_mount(key) {
            let rsa_key = self.rsa.clone().context("asymmetric encryption requested but no RSA key is configured")?;
            let (cipher, pre) = v3_new_key(&rsa_key)?;
            // Plaintext source: the staging file itself, or (staged under the
            // master key) a decrypting read of it.
            let mut w = self.begin_stage_with_pre(Some(cipher), &pre).await?;
            let res: Result<()> = async {
                if staged.encrypted {
                    let tmp = format!("_v3-tmp/{}", crate::Id::new());
                    self.backend.put_path(&tmp, &staged.path).await?;
                    let r = async {
                        let mut stream = self.open_stream(&tmp, true).await?.context("staged object vanished")?;
                        while let Some(c) = stream.next().await { w.write(&c?).await?; }
                        Ok::<(), anyhow::Error>(())
                    }.await;
                    let _ = self.backend.delete(&tmp).await;
                    r
                } else {
                    use tokio::io::AsyncReadExt;
                    let mut f = tokio::fs::File::open(&staged.path).await?;
                    let mut buf = vec![0u8; 256 * 1024];
                    loop {
                        let n = f.read(&mut buf).await?;
                        if n == 0 { break; }
                        w.write(&buf[..n]).await?;
                    }
                    Ok(())
                }
            }.await;
            if let Err(e) = res { w.abort().await; return Err(e); }
            let st2 = w.finish().await?;
            let _ = tokio::fs::remove_file(&staged.path).await;
            return Ok((self.commit(st2, key).await?, true));
        }
        if is_mount(key) {
            if !staged.encrypted { return Ok((self.commit(staged, key).await?, false)); }
            // Staged encrypted (encrypt-all): a mounted folder holds plain files,
            // so decrypt it into a plain staging file first.
            let tmp = format!("_mount-tmp/{}", crate::Id::new());
            self.backend.put_path(&tmp, &staged.path).await?;
            let plain = async {
                let mut stream = self.open_stream(&tmp, true).await?.context("staged object vanished")?;
                let mut w = self.begin_stage_with(None).await?;
                while let Some(c) = stream.next().await {
                    let c = c?;
                    if let Err(e) = w.write(&c).await { w.abort().await; return Err(e); }
                }
                w.finish().await
            }.await;
            let _ = self.backend.delete(&tmp).await;
            return Ok((self.commit(plain?, key).await?, false));
        }
        if !encrypt || staged.encrypted {
            let enc = staged.encrypted;
            return Ok((self.commit(staged, key).await?, enc));
        }
        let cipher = self.cipher.clone().context("encryption requested but STORAGE_ENC_KEY is not set")?;
        let mut w = self.begin_stage_with(Some(cipher)).await?;
        let mut f = tokio::fs::File::open(&staged.path).await?;
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            use tokio::io::AsyncReadExt;
            let n = f.read(&mut buf).await?;
            if n == 0 { break; }
            if let Err(e) = w.write(&buf[..n]).await { w.abort().await; return Err(e); }
        }
        let st2 = w.finish().await?;
        let _ = tokio::fs::remove_file(&staged.path).await;
        Ok((self.commit(st2, key).await?, true))
    }

    async fn begin_stage_with(&self, cipher: Option<Aes256Gcm>) -> Result<StageWriter> {
        self.begin_stage_with_pre(cipher, &[]).await
    }

    /// Like `begin_stage_with`, writing `pre` (the v3 preamble) before the v2 header.
    async fn begin_stage_with_pre(&self, cipher: Option<Aes256Gcm>, pre: &[u8]) -> Result<StageWriter> {
        let dir = self.backend.staging_dir();
        tokio::fs::create_dir_all(&dir).await?;
        let sid = crate::Id::new().to_string();
        let dir = dir.join(shard(&sid));
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("up-{sid}"));
        let f = tokio::fs::File::create(&path).await?;
        let mut prefix = [0u8; 7];
        rand::Rng::fill(&mut rand::thread_rng(), &mut prefix[..]);
        let header = v2_header(&prefix);
        let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, f);
        if cipher.is_some() { file.write_all(pre).await?; file.write_all(&header).await?; }
        Ok(StageWriter { file, path, cipher, prefix, header, idx: 0, buf: BytesMut::with_capacity(V2_CHUNK * 2), md5: Md5::new(), size: 0, head: Vec::new(), clam: None })
    }

    /// Put with an explicit encryption choice. Returns (etag, encrypted).
    pub async fn put_as(&self, key: &str, body: Bytes, encrypt: bool) -> Result<(String, bool)> {
        self.put_mode(key, body, encrypt, false).await
    }

    /// [`Storage::put_as`] with a choice of asymmetric (RSA, format v3) encryption.
    pub async fn put_mode(&self, key: &str, body: Bytes, encrypt: bool, asymmetric: bool) -> Result<(String, bool)> {
        let encrypt = encrypt && !is_mount(key);
        if encrypt && asymmetric {
            let rsa_key = self.rsa.clone().context("asymmetric encryption requested but no RSA key is configured")?;
            let mut h = Md5::new();
            h.update(&body);
            let etag = hex::encode(h.finalize());
            let (cipher, mut out) = v3_new_key(&rsa_key)?;
            out.extend_from_slice(&encrypt_v2(&cipher, &body)?);
            self.be(key)?.put(Self::k(key), Bytes::from(out)).await?;
            return Ok((etag, true));
        }
        let mut h = Md5::new();
        h.update(&body);
        let etag = hex::encode(h.finalize());
        let enc = encrypt && self.cipher.is_some();
        if encrypt && !enc { anyhow::bail!("encryption requested but STORAGE_ENC_KEY is not set"); }
        let on_disk = if enc { Bytes::from(encrypt_v2(self.cipher.as_ref().unwrap(), &body)?) } else { body };
        self.be(key)?.put(Self::k(key), on_disk).await?;
        Ok((etag, enc))
    }

    pub async fn discard(&self, staged: Staged) {
        let _ = tokio::fs::remove_file(&staged.path).await;
    }

    /// Write the object, returning an MD5 of the **plaintext** so the ETag
    /// stays stable across re-keying.  New encrypted objects use format v2.
    pub async fn put(&self, key: &str, body: Bytes, _content_type: Option<&str>) -> Result<String> {
        let mut h = Md5::new();
        h.update(&body);
        let etag = hex::encode(h.finalize());

        let on_disk = match (&self.cipher, self.encrypt_all) {
            (Some(cipher), true) => Bytes::from(encrypt_v2(cipher, &body)?),
            _ => body,
        };

        self.be(key)?.put(Self::k(key), on_disk).await?;
        Ok(etag)
    }

    // ---- read ------------------------------------------------------------

    /// Size + layout of an object without reading its body.
    pub async fn stat(&self, key: &str, encrypted: bool) -> Result<Option<ObjInfo>> {
        let Some(stored) = self.be(key)?.size(Self::k(key)).await? else { return Ok(None) };
        if !encrypted {
            return Ok(Some(ObjInfo { size: stored, stored, layout: Layout::Plain }));
        }
        let head = self.read_raw_prefix(key, V2_HEADER as u64).await?;
        if head.len() >= V3_FIXED && &head[..8] == V3_MAGIC {
            let wl = u16::from_le_bytes([head[8], head[9]]) as usize;
            let base = V3_FIXED + wl;
            let all = self.read_raw_prefix(key, (base + V2_HEADER) as u64).await?;
            if all.len() < base + V2_HEADER || &all[base..base + 8] != V2_MAGIC { anyhow::bail!("corrupt v3 header: {key}"); }
            let dk = v3_unwrap(self.rsa.as_deref(), &all[V3_FIXED..base])?;
            let h = &all[base..base + V2_HEADER];
            let chunk = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
            if chunk == 0 { anyhow::bail!("corrupt v3 header (chunk size 0): {key}"); }
            let mut prefix = [0u8; 7];
            prefix.copy_from_slice(&h[12..19]);
            let mut header = [0u8; V2_HEADER];
            header.copy_from_slice(h);
            let body = stored.saturating_sub((base + V2_HEADER) as u64);
            let full = (chunk + V2_TAG) as u64;
            let chunks = body.div_ceil(full).max(1);
            let size = body.checked_sub(chunks * V2_TAG as u64)
                .with_context(|| format!("corrupt v3 object (too short): {key}"))?;
            return Ok(Some(ObjInfo { size, stored, layout: Layout::V3 { chunk, chunks, prefix, header, base: base as u64, dk } }));
        }
        if head.len() == V2_HEADER && &head[..8] == V2_MAGIC {
            let chunk = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
            if chunk == 0 { anyhow::bail!("corrupt v2 header (chunk size 0): {key}"); }
            let mut prefix = [0u8; 7];
            prefix.copy_from_slice(&head[12..19]);
            let mut header = [0u8; V2_HEADER];
            header.copy_from_slice(&head);
            let body = stored.saturating_sub(V2_HEADER as u64);
            let full = (chunk + V2_TAG) as u64;
            let chunks = body.div_ceil(full).max(1);
            let size = body.checked_sub(chunks * V2_TAG as u64)
                .with_context(|| format!("corrupt v2 object (too short): {key}"))?;
            return Ok(Some(ObjInfo { size, stored, layout: Layout::V2 { chunk, chunks, prefix, header } }));
        }
        Ok(Some(ObjInfo { size: stored.saturating_sub(12 + V2_TAG as u64), stored, layout: Layout::V1 }))
    }

    /// The whole object as plaintext, streamed with O(chunk) memory.
    pub async fn open_stream(&self, key: &str, encrypted: bool) -> Result<Option<ByteStream>> {
        let Some(info) = self.stat(key, encrypted).await? else { return Ok(None) };
        Ok(Some(self.read_range(key, &info, 0, info.size.saturating_sub(1)).await?))
    }

    async fn read_raw_prefix(&self, key: &str, n: u64) -> Result<Bytes> {
        let Some(mut s) = self.be(key)?.open_range(Self::k(key), 0, n).await? else { return Ok(Bytes::new()) };
        let mut out = BytesMut::new();
        while let Some(c) = s.next().await {
            out.extend_from_slice(&c?);
        }
        Ok(out.freeze())
    }

    /// Stream plaintext bytes `start..=end` (inclusive, already clamped by the
    /// caller against `info.size`).  Plain and v2 objects stream with O(chunk)
    /// memory; v1 objects fall back to one whole-object decrypt.
    pub async fn read_range(&self, key: &str, info: &ObjInfo, start: u64, end: u64) -> Result<ByteStream> {
        if info.size == 0 || start > end {
            return Ok(Box::pin(futures::stream::empty()));
        }
        match info.layout {
            Layout::Plain => self.be(key)?.open_range(Self::k(key), start, end - start + 1).await?
                .with_context(|| format!("object vanished: {key}")),
            Layout::V1 => {
                let (all, _) = self.get(key, true).await?.with_context(|| format!("object vanished: {key}"))?;
                let e = ((end + 1) as usize).min(all.len());
                let part = all.slice((start as usize).min(e)..e);
                Ok(Box::pin(futures::stream::once(async move { Ok(part) })))
            }
            Layout::V2 { chunk, chunks, prefix, header } => {
                let cipher = self.cipher.clone()
                    .context("file is flagged encrypted but STORAGE_ENC_KEY is not configured")?;
                let full = (chunk + V2_TAG) as u64;
                let first = start / chunk as u64;
                let last = end / chunk as u64;
                let off = V2_HEADER as u64 + first * full;
                let len = (last - first + 1) * full;
                let raw = self.be(key)?.open_range(Self::k(key), off, len).await?
                    .with_context(|| format!("object vanished: {key}"))?;
                Ok(decrypt_v2_stream(raw, cipher, prefix, header, chunk, chunks, first,
                                     start % chunk as u64, end - start + 1))
            }
            Layout::V3 { chunk, chunks, prefix, header, base, dk } => {
                let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dk));
                let full = (chunk + V2_TAG) as u64;
                let first = start / chunk as u64;
                let last = end / chunk as u64;
                let off = base + V2_HEADER as u64 + first * full;
                let len = (last - first + 1) * full;
                let raw = self.be(key)?.open_range(Self::k(key), off, len).await?
                    .with_context(|| format!("object vanished: {key}"))?;
                Ok(decrypt_v2_stream(raw, cipher, prefix, header, chunk, chunks, first,
                                     start % chunk as u64, end - start + 1))
            }
        }
    }

    /// Read the whole object into memory.  Kept for small-object callers
    /// (previews, WOPI, text extraction); downloads use [`read_range`].
    pub async fn get(&self, key: &str, encrypted: bool) -> Result<Option<(Bytes, Option<String>)>> {
        let raw = match self.be(key)?.get(Self::k(key)).await? {
            Some(b) => b,
            None    => return Ok(None),
        };

        let plaintext = if !encrypted {
            raw
        } else if raw.len() >= V3_FIXED && &raw[..8] == V3_MAGIC {
            let base = V3_FIXED + u16::from_le_bytes([raw[8], raw[9]]) as usize;
            if raw.len() < base + V2_HEADER { anyhow::bail!("encrypted blob too short"); }
            let dk = v3_unwrap(self.rsa.as_deref(), &raw[V3_FIXED..base])?;
            Bytes::from(decrypt_v2_all(&Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dk)), &raw[base..])?)
        } else {
            let cipher = self.cipher.as_ref()
                .context("file is flagged encrypted but STORAGE_ENC_KEY is not configured")?;
            if raw.len() >= V2_HEADER && &raw[..8] == V2_MAGIC {
                Bytes::from(decrypt_v2_all(cipher, &raw)?)
            } else {
                if raw.len() < 12 {
                    anyhow::bail!("encrypted blob too short");
                }
                let (nonce_bytes, ct) = raw.split_at(12);
                let nonce = Nonce::from_slice(nonce_bytes);
                Bytes::from(cipher
                    .decrypt(nonce, ct)
                    .map_err(|e| anyhow::anyhow!("aes-gcm decrypt: {e}"))?)
            }
        };

        let ct = mime_guess::from_path(key).first().map(|m| m.to_string());
        Ok(Some((plaintext, ct)))
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        self.be(key)?.delete(Self::k(key)).await
    }

    /// Move an object to another key without re-encrypting (the key is not
    /// part of the v2 AAD) — a rename on the filesystem backend.
    pub async fn rename(&self, from: &str, to: &str) -> Result<()> {
        if is_mount(from) == is_mount(to) {
            self.be(to)?;
            return self.be(from)?.rename(Self::k(from), Self::k(to)).await;
        }
        // Between a mounted folder and the main store: copy the stored bytes, then remove.
        let dst = self.be(to)?;
        let Some(mut src) = self.be(from)?.open_range(Self::k(from), 0, u64::MAX).await? else { return Ok(()) };
        let dir = self.backend.staging_dir();
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("mv-{}", crate::Id::new()));
        {
            let mut f = tokio::fs::File::create(&path).await?;
            while let Some(c) = src.next().await { f.write_all(&c?).await?; }
            f.flush().await?;
        }
        dst.put_path(Self::k(to), &path).await?;
        self.be(from)?.delete(Self::k(from)).await
    }
}

pub const MOUNT: &str = "mount:";
/// Keys of files that live in a folder mounted into a basket.
pub fn is_mount(key: &str) -> bool { key.starts_with(MOUNT) }

fn encrypt_v2(cipher: &Aes256Gcm, plain: &[u8]) -> Result<Vec<u8>> {
    let mut prefix = [0u8; 7];
    rand::Rng::fill(&mut rand::thread_rng(), &mut prefix[..]);
    let header = v2_header(&prefix);
    let n = plain.len().div_ceil(V2_CHUNK).max(1);
    let mut out = Vec::with_capacity(V2_HEADER + plain.len() + n * V2_TAG);
    out.extend_from_slice(&header);
    for i in 0..n {
        let s = i * V2_CHUNK;
        let e = (s + V2_CHUNK).min(plain.len());
        let nonce = v2_nonce(&prefix, i as u32, i == n - 1);
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: &plain[s.min(e)..e], aad: &header })
            .map_err(|e| anyhow::anyhow!("aes-gcm encrypt: {e}"))?;
        out.extend_from_slice(&ct);
    }
    Ok(out)
}

fn decrypt_v2_all(cipher: &Aes256Gcm, raw: &[u8]) -> Result<Vec<u8>> {
    let header = &raw[..V2_HEADER];
    let chunk = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if chunk == 0 { anyhow::bail!("corrupt v2 header"); }
    let mut prefix = [0u8; 7];
    prefix.copy_from_slice(&header[12..19]);
    let body = &raw[V2_HEADER..];
    let full = chunk + V2_TAG;
    let n = body.len().div_ceil(full).max(1);
    let mut out = Vec::with_capacity(body.len());
    for i in 0..n {
        let s = i * full;
        let e = (s + full).min(body.len());
        let nonce = v2_nonce(&prefix, i as u32, i == n - 1);
        let pt = cipher
            .decrypt(Nonce::from_slice(&nonce), Payload { msg: &body[s.min(e)..e], aad: header })
            .map_err(|_| anyhow::anyhow!("aes-gcm decrypt: chunk {i} failed authentication"))?;
        out.extend_from_slice(&pt);
    }
    Ok(out)
}

/// Turn a stream of v2 ciphertext (starting at chunk `first_idx`) into a
/// stream of plaintext, dropping `skip` bytes at the front and stopping after
/// `want` bytes.
#[allow(clippy::too_many_arguments)]
fn decrypt_v2_stream(
    raw: ByteStream, cipher: Aes256Gcm, prefix: [u8; 7], header: [u8; V2_HEADER],
    chunk: usize, chunks: u64, first_idx: u64, skip: u64, want: u64,
) -> ByteStream {
    struct St {
        raw: ByteStream, cipher: Aes256Gcm, prefix: [u8; 7], header: [u8; V2_HEADER],
        full: usize, chunks: u64, idx: u64, skip: u64, left: u64, buf: BytesMut, eof: bool,
    }
    let st = St {
        raw, cipher, prefix, header, full: chunk + V2_TAG, chunks,
        idx: first_idx, skip, left: want, buf: BytesMut::new(), eof: false,
    };
    // Decrypt up to BATCH chunks per yielded item: one 64 KiB frame per poll
    // made full-file downloads ~2x slower than the old whole-object path.
    const BATCH: usize = 16;
    Box::pin(futures::stream::unfold(st, |mut st| async move {
        if st.left == 0 { return None; }
        while st.buf.len() < st.full * BATCH && !st.eof {
            match st.raw.next().await {
                Some(Ok(b)) => st.buf.extend_from_slice(&b),
                Some(Err(e)) => return Some((Err(e), St { left: 0, ..st })),
                None => st.eof = true,
            }
        }
        if st.buf.is_empty() {
            let e = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "v2 object truncated");
            return Some((Err(e), St { left: 0, ..st }));
        }
        let mut out = BytesMut::with_capacity((st.buf.len() / st.full + 1) * (st.full - V2_TAG));
        while !st.buf.is_empty() && (st.buf.len() >= st.full || st.eof) && st.left > 0 {
            let take = st.full.min(st.buf.len());
            let ct = st.buf.split_to(take);
            let last = st.idx + 1 == st.chunks;
            let nonce = v2_nonce(&st.prefix, st.idx as u32, last);
            let pt = match st.cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: &st.header }) {
                Ok(p) => p,
                Err(_) => {
                    let e = std::io::Error::new(std::io::ErrorKind::InvalidData,
                        format!("aes-gcm decrypt: chunk {} failed authentication", st.idx));
                    return Some((Err(e), St { left: 0, ..st }));
                }
            };
            st.idx += 1;
            let mut pt = &pt[..];
            if st.skip > 0 {
                let k = (st.skip as usize).min(pt.len());
                pt = &pt[k..];
                st.skip -= k as u64;
            }
            if pt.len() as u64 > st.left {
                pt = &pt[..st.left as usize];
            }
            st.left -= pt.len() as u64;
            out.extend_from_slice(pt);
        }
        Some((Ok(out.freeze()), st))
    }))
}

// =============================================================================
// Unit tests — encrypt/decrypt invariants across whichever backend.
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{AeadCore, OsRng};
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

    async fn collect(s: ByteStream) -> Vec<u8> {
        let mut s = s;
        let mut v = Vec::new();
        while let Some(c) = s.next().await { v.extend_from_slice(&c.unwrap()); }
        v
    }

    fn pattern(n: usize) -> Vec<u8> { (0..n).map(|i| (i * 7 + i / 251) as u8).collect() }

    #[tokio::test]
    async fn v2_stage_and_ranges_match_plaintext() {
        for enc in [true, false] {
            let s = make(if enc { "v2enc" } else { "v2plain" }, enc).await;
            // sizes around chunk boundaries, incl. empty and exact multiples
            for n in [0usize, 1, V2_CHUNK - 1, V2_CHUNK, V2_CHUNK + 1, 3 * V2_CHUNK, 3 * V2_CHUNK + 777] {
                let data = pattern(n);
                let mut w = s.begin_stage().await.unwrap();
                for part in data.chunks(10_000) { w.write(part).await.unwrap(); }
                let staged = w.finish().await.unwrap();
                assert_eq!(staged.size, n as u64);
                let key = format!("b/obj-{n}");
                s.commit(staged, &key).await.unwrap();
                let info = s.stat(&key, enc).await.unwrap().unwrap();
                assert_eq!(info.size, n as u64, "size n={n} enc={enc}");
                if enc { assert!(matches!(info.layout, Layout::V2 { .. })); }
                let (all, _) = s.get(&key, enc).await.unwrap().unwrap();
                assert_eq!(&all[..], &data[..]);
                if n == 0 { continue; }
                for (a, b) in [(0, n - 1), (n / 2, n - 1), (0, 0), (n - 1, n - 1),
                               (V2_CHUNK.min(n - 1), (2 * V2_CHUNK + 5).min(n - 1)), (n / 3, n / 3 + 100)] {
                    let b = b.min(n - 1);
                    if a > b { continue; }
                    let got = collect(s.read_range(&key, &info, a as u64, b as u64).await.unwrap()).await;
                    assert_eq!(got, &data[a..=b], "range {a}-{b} n={n} enc={enc}");
                }
            }
        }
    }

    #[tokio::test]
    async fn v2_put_is_readable_and_tamper_detected() {
        let s = make("v2tamper", true).await;
        let data = pattern(2 * V2_CHUNK + 10);
        s.put("b/t", Bytes::from(data.clone()), None).await.unwrap();
        let (got, _) = s.get("b/t", true).await.unwrap().unwrap();
        assert_eq!(&got[..], &data[..]);
        // flip one byte in the second chunk
        let mut raw = s.backend.get("b/t").await.unwrap().unwrap().to_vec();
        let pos = V2_HEADER + V2_CHUNK + V2_TAG + 5;
        raw[pos] ^= 1;
        s.backend.put("b/t", Bytes::from(raw)).await.unwrap();
        assert!(s.get("b/t", true).await.is_err());
        // truncation (drop last chunk) is detected too
        let raw = s.backend.get("b/t").await.unwrap().unwrap();
        let cut = raw.slice(..V2_HEADER + 2 * (V2_CHUNK + V2_TAG));
        s.backend.put("b/t", cut).await.unwrap();
        assert!(s.get("b/t", true).await.is_err());
    }

    #[tokio::test]
    async fn v1_objects_still_readable() {
        let s = make("v1compat", true).await;
        let cipher = s.cipher.clone().unwrap();
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let data = pattern(100_000);
        let mut raw = nonce.to_vec();
        raw.extend_from_slice(&cipher.encrypt(&nonce, data.as_ref()).unwrap());
        s.backend.put("b/v1", Bytes::from(raw)).await.unwrap();
        let info = s.stat("b/v1", true).await.unwrap().unwrap();
        assert_eq!(info.layout, Layout::V1);
        assert_eq!(info.size, data.len() as u64);
        let got = collect(s.read_range("b/v1", &info, 10, 99).await.unwrap()).await;
        assert_eq!(got, &data[10..=99]);
    }

    #[tokio::test]
    async fn encryption_disabled_when_no_key() {
        let s = make("noenc", false).await;
        assert!(!s.encryption_enabled());
    }
}

/// Work area for uploads in progress (TUS chunks, S3 parts). `SPOOL_DIR` when
/// set — it must be one volume shared by every pod — else `STORAGE_ROOT`.
pub fn spool_root() -> std::path::PathBuf {
    std::env::var("SPOOL_DIR").ok().filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("STORAGE_ROOT").ok())
        .unwrap_or_else(|| "./storage".into()).into()
}

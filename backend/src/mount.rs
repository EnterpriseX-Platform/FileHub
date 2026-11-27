//! Mounted folders — a basket whose files are the files of a folder on disk
//! (a Docker volume, NFS or SMB share mounted into the pod).
//!
//! The folder stays the source of truth: files other programs put there show
//! up in the basket after the next scan, uploads to the basket are written into
//! the folder under their real names, and files removed from the folder leave
//! the basket. Only folders under `MOUNT_ROOTS` (comma-separated absolute
//! paths) may be mounted, so an admin can't expose `/etc` or the storage root.
//!
//! Files are kept as plain files (other programs must be able to read them).
//! Older versions are moved into FileHub's own storage, so the folder always
//! holds the current version and history is still kept.

use std::collections::HashMap;
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::{extract::{Path, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use crate::Id;

use crate::auth::AdminUser;
use crate::config::env_opt;
use crate::error::{ApiError, ApiResult};
use crate::models::System;
use crate::storage::MOUNT;
use crate::AppState;

fn bad(m: impl Into<String>) -> ApiError { ApiError::BadRequest(m.into()) }

fn roots() -> Vec<PathBuf> {
    env_opt("MOUNT_ROOTS").unwrap_or_default().split(',').map(str::trim)
        .filter(|r| r.starts_with('/'))
        // Resolved, so a root reached through a symlink (e.g. /tmp on macOS) still matches.
        .map(|r| std::fs::canonicalize(r).unwrap_or_else(|_| PathBuf::from(r.trim_end_matches('/')))).collect()
}

/// A path FileHub may read or write as a mounted file: absolute, no `..`,
/// and inside one of MOUNT_ROOTS.
pub fn check_path(p: &str) -> anyhow::Result<()> {
    let path = FsPath::new(p);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        anyhow::bail!("invalid mounted path");
    }
    if !roots().iter().any(|r| path.starts_with(r) && path != r.as_path()) {
        anyhow::bail!("path is outside MOUNT_ROOTS");
    }
    Ok(())
}

/// The storage key for a new upload: inside the mounted folder under its real
/// name when the basket is mounted (a number is added if the name is taken),
/// else FileHub's own `bucket/id-name`.
pub async fn object_key_for(db: &PgPool, system: &System, folder_id: Option<&str>, id: Id, name: &str) -> ApiResult<String> {
    let safe = crate::handlers::sanitize_filename(name);
    let Some(root) = system.mount_path.as_deref() else {
        let id = id.to_string();
        return Ok(format!("{}/{}/{}-{}", system.bucket, crate::storage::shard(&id), id, safe));
    };
    let mut dir = PathBuf::from(root);
    for part in folder_chain(db, folder_id).await? { dir.push(crate::handlers::sanitize_filename(&part)); }
    // The real directory, so a symlink planted in the share can't lead outside MOUNT_ROOTS.
    tokio::fs::create_dir_all(&dir).await.map_err(|e| bad(format!("Can't write to the folder: {e}")))?;
    let dir = tokio::fs::canonicalize(&dir).await.map_err(|e| bad(format!("Can't write to the folder: {e}")))?;
    check_path(&dir.join("x").to_string_lossy()).map_err(|_| bad("That folder leads outside the mounted area."))?;
    let (stem, ext) = match safe.rfind('.') { Some(i) if i > 0 => (&safe[..i], &safe[i..]), _ => (safe.as_str(), "") };
    for n in 1..1000 {
        let candidate = if n == 1 { dir.join(&safe) } else { dir.join(format!("{stem} ({n}){ext}")) };
        if !tokio::fs::try_exists(&candidate).await.unwrap_or(true) {
            let p = candidate.to_string_lossy().to_string();
            check_path(&p).map_err(|e| bad(e.to_string()))?;
            return Ok(format!("{MOUNT}{p}"));
        }
    }
    Err(bad("Too many files with this name in the folder."))
}

/// Folder names from the basket root down to `folder_id`.
async fn folder_chain(db: &PgPool, folder_id: Option<&str>) -> ApiResult<Vec<String>> {
    let Some(fid) = folder_id else { return Ok(vec![]) };
    let names: Vec<String> = sqlx::query_scalar(
        r#"WITH RECURSIVE up AS (
             SELECT id, parent_id, name, 0 AS depth FROM folders WHERE id = $1
             UNION ALL SELECT f.id, f.parent_id, f.name, up.depth + 1 FROM folders f JOIN up ON f.id = up.parent_id WHERE up.depth < 64)
           SELECT name FROM up ORDER BY depth DESC"#,
    ).bind(fid).fetch_all(db).await?;
    Ok(names)
}

/// Keys for a new version: `(where the old version is kept, where the new one goes)`.
/// In a mounted folder the current file keeps its place, so the old bytes are
/// moved into FileHub's storage first; elsewhere every version gets its own key.
pub async fn version_keys(s: &AppState, current: &str, bucket: &str, id: Id, old_version: i64, new_version: i64, safe: &str) -> ApiResult<(String, String)> {
    if crate::storage::is_mount(current) {
        let archive = format!("{bucket}/{id}-v{old_version}-{safe}");
        s.storage.rename(current, &archive).await?;
        Ok((archive, current.to_string()))
    } else {
        Ok((current.to_string(), format!("{bucket}/{id}-v{new_version}-{safe}")))
    }
}

// ── Scan ──

struct Entry { dir: Vec<String>, name: String, path: String, size: i64, mtime: i64 }

/// Every regular file under `root` (hidden files, temp files and symlinks skipped).
fn walk(root: &FsPath) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), Vec::<String>::new())];
    while let Some((dir, rel)) = stack.pop() {
        if rel.len() > 32 { continue; }
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name.contains(".tmp.") { continue; }
            let md = e.path().symlink_metadata()?;
            if md.file_type().is_symlink() { continue; }
            if md.is_dir() {
                let mut r = rel.clone(); r.push(name);
                stack.push((e.path(), r));
            } else if md.is_file() {
                let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                out.push(Entry { dir: rel.clone(), name, path: e.path().to_string_lossy().to_string(), size: md.len() as i64, mtime });
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ScanResult { pub added: usize, pub updated: usize, pub removed: usize }

/// Bring a mounted basket in line with its folder.
pub async fn scan(s: &AppState, system: &System) -> ApiResult<ScanResult> {
    let Some(root) = system.mount_path.clone() else { return Err(bad("This basket isn't a mounted folder.")) };
    check_path(&format!("{root}/x")).map_err(|e| bad(e.to_string()))?;
    let entries = tokio::task::spawn_blocking({ let r = root.clone(); move || walk(FsPath::new(&r)) })
        .await.map_err(|e| ApiError::Other(e.into()))?
        .map_err(|e| bad(format!("Can't read the folder: {e}")))?;

    let known: Vec<(Id, String, Option<String>, bool)> = sqlx::query_as(
        "SELECT id, object_key, etag, deleted_at IS NOT NULL FROM files WHERE system_id = $1 AND object_key LIKE 'mount:%'",
    ).bind(&system.id).fetch_all(&s.db).await?;
    let by_key: HashMap<&str, (Id, Option<&str>, bool)> = known.iter().map(|(id, k, e, d)| (k.as_str(), (*id, e.as_deref(), *d))).collect();
    let mut folders: HashMap<Vec<String>, Option<String>> = HashMap::from([(vec![], None)]);
    let mut seen = std::collections::HashSet::new();
    let mut r = ScanResult::default();
    let initial: String = sqlx::query_scalar("SELECT code FROM basket_statuses WHERE system_id = $1 AND is_initial LIMIT 1")
        .bind(&system.id).fetch_optional(&s.db).await?.unwrap_or_else(|| "Draft".into());

    for e in entries {
        let key = format!("{MOUNT}{}", e.path);
        seen.insert(key.clone());
        let stamp = format!("m{}-{}", e.mtime, e.size);
        match by_key.get(key.as_str()) {
            // Unchanged — including files someone moved to Trash in FileHub.
            Some((_, Some(et), _)) if *et == stamp => {}
            Some((id, _, _)) => {
                // Changed on disk: refresh it.
                sqlx::query("UPDATE files SET size_bytes = $2, etag = $3, deleted_at = NULL, modified_at = to_timestamp($4) WHERE id = $1")
                    .bind(id).bind(e.size).bind(&stamp).bind(e.mtime as f64).execute(&s.db).await?;
                sqlx::query("DELETE FROM file_previews WHERE file_id = $1").bind(id).execute(&s.db).await.ok();
                crate::handlers::spawn_p1(s, *id, crate::handlers::detect_file_type(&e.name), e.name.clone(), key, false, e.size as u64);
                r.updated += 1;
            }
            None => {
                let folder = folder_for(&s.db, &system.id, &e.dir, &mut folders).await?;
                let id = crate::Id::new();
                let ft = crate::handlers::detect_file_type(&e.name);
                sqlx::query(
                    r#"INSERT INTO files (id, name, file_type, size_bytes, system_id, folder_id, bucket, object_key, status, owner, tags,
                                         version, metadata, etag, encrypted, created_at, modified_at)
                       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'Mounted folder','[]',1,'{}',$10,false,to_timestamp($11),to_timestamp($11))"#,
                ).bind(id).bind(&e.name).bind(&ft).bind(e.size).bind(&system.id).bind(&folder).bind(&system.bucket)
                 .bind(&key).bind(&initial).bind(&stamp).bind(e.mtime as f64).execute(&s.db).await?;
                crate::handlers::spawn_p1(s, id, ft, e.name.clone(), key, false, e.size as u64);
                r.added += 1;
            }
        }
    }
    // Gone from the folder: out of the basket too (to Trash, so a mistake can be undone
    // by putting the file back).
    for (id, k, _, deleted) in &known {
        if !deleted && !seen.contains(k) {
            sqlx::query("UPDATE files SET deleted_at = now() WHERE id = $1").bind(id).execute(&s.db).await?;
            r.removed += 1;
        }
    }
    sqlx::query("UPDATE systems SET mount_scanned_at = now() WHERE id = $1").bind(&system.id).execute(&s.db).await?;
    Ok(r)
}

/// The basket folder for a directory path, made on first sight.
async fn folder_for(db: &PgPool, sys: &str, dir: &[String], cache: &mut HashMap<Vec<String>, Option<String>>) -> ApiResult<Option<String>> {
    if let Some(f) = cache.get(dir) { return Ok(f.clone()); }
    let parent = Box::pin(folder_for(db, sys, &dir[..dir.len() - 1], cache)).await?;
    let name = &dir[dir.len() - 1];
    let found: Option<String> = sqlx::query_scalar(
        "SELECT id FROM folders WHERE system_id = $1 AND parent_id IS NOT DISTINCT FROM $2 AND name = $3 AND deleted_at IS NULL LIMIT 1",
    ).bind(sys).bind(&parent).bind(name).fetch_optional(db).await?;
    let id = match found {
        Some(id) => id,
        None => {
            let id = crate::new_id();
            sqlx::query("INSERT INTO folders (id, system_id, parent_id, name, owner) VALUES ($1,$2,$3,$4,'Mounted folder')")
                .bind(&id).bind(sys).bind(&parent).bind(name).execute(db).await?;
            id
        }
    };
    cache.insert(dir.to_vec(), Some(id.clone()));
    Ok(Some(id))
}

/// Scan every mounted basket now and then (MOUNT_SCAN_SECS, default 300; 0 = off).
pub async fn run_worker(s: Arc<AppState>) {
    let every = crate::config::env_num("MOUNT_SCAN_SECS", 300);
    if every == 0 { return; }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(every)).await;
        // One pod at a time: the others skip this round.
        let Ok(mut tx) = s.db.begin().await else { continue };
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(7311001)").fetch_one(&mut *tx).await.unwrap_or(false);
        if !got { continue; }
        let baskets: Vec<System> = sqlx::query_as("SELECT * FROM systems WHERE mount_path IS NOT NULL AND deleted_at IS NULL")
            .fetch_all(&s.db).await.unwrap_or_default();
        for b in &baskets {
            if let Err(e) = scan(&s, b).await { tracing::warn!(basket = %b.id, "mounted folder scan failed: {e:?}"); }
        }
        let _ = tx.commit().await;
    }
}

// ── Admin REST ──

#[derive(Deserialize)]
pub struct SetMount { pub path: Option<String> }

/// PUT /api/baskets/:id/mount {path} — mount a folder (null = stop).
pub async fn set_mount(State(s): State<Arc<AppState>>, _: AdminUser, Path(id): Path<String>, Json(b): Json<SetMount>) -> ApiResult<Json<Value>> {
    let system: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    let mut path = b.path.map(|p| p.trim().trim_end_matches('/').to_string()).filter(|p| !p.is_empty());
    if let Some(p) = &path {
        // Judge the real folder, not a symlink that points somewhere else.
        let real = tokio::fs::canonicalize(p).await.map_err(|_| bad("That folder doesn't exist on the server."))?;
        check_path(&real.to_string_lossy()).map_err(|_| bad("Choose a folder inside the mounted area."))?;
    }
    if let Some(p) = path.take() { path = Some(tokio::fs::canonicalize(&p).await.map(|r| r.to_string_lossy().to_string()).unwrap_or(p)); }
    if let Some(p) = &path {
        if system.vault { return Err(bad("A Vault can't be a mounted folder: its files must stay encrypted.")); }
        if roots().is_empty() { return Err(bad("No folders may be mounted on this server (MOUNT_ROOTS is empty).")); }
        check_path(p).map_err(|_| bad(format!("Choose a folder inside {}.", roots().iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", "))))?;
        let md = tokio::fs::metadata(p).await.map_err(|_| bad("That folder doesn't exist on the server."))?;
        if !md.is_dir() { return Err(bad("That path isn't a folder.")); }
        let taken: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM systems WHERE id <> $1 AND deleted_at IS NULL AND mount_path IS NOT NULL
                                                AND ($2 LIKE mount_path || '/%' OR mount_path LIKE $2 || '/%' OR mount_path = $2))")
            .bind(&id).bind(p).fetch_one(&s.db).await?;
        if taken { return Err(bad("Another basket already uses this folder or one inside it.")); }
    }
    sqlx::query("UPDATE systems SET mount_path = $2 WHERE id = $1").bind(&id).bind(&path).execute(&s.db).await?;
    let result = match &path {
        Some(_) => {
            let system: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1").bind(&id).fetch_one(&s.db).await?;
            Some(scan(&s, &system).await?)
        }
        None => None,
    };
    Ok(Json(json!({ "mount_path": path, "scan": result })))
}

/// POST /api/baskets/:id/scan — read the folder now (Manage on the basket).
pub async fn scan_now(State(s): State<Arc<AppState>>, user: crate::auth::AuthUser, Path(id): Path<String>) -> ApiResult<Json<ScanResult>> {
    crate::acl::require_folder(&s.db, &user.0, &id, None, crate::acl::MANAGE).await?;
    let system: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(scan(&s, &system).await?))
}

/// GET /api/mount-roots — the folders an admin may choose from.
pub async fn list_roots(_: AdminUser) -> Json<Vec<String>> {
    Json(roots().iter().map(|r| r.display().to_string()).collect())
}

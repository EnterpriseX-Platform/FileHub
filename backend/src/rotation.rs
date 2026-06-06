//! Phase K — rotation engine.
//!
//! Periodically (and on demand) the worker reads `rotation_policies` and:
//!   1. **Version pruning** — for each file with more than `keep_last_n_versions`
//!      historical versions, drops the oldest ones (both DB row and the
//!      backing object on disk) so a single noisy file can't balloon storage.
//!   2. **Archive** — files older than `archive_after_days` get soft-deleted
//!      (`deleted_at = now()`), which moves them into the Trash view without
//!      yet freeing storage.
//!   3. **Hard delete** — soft-deleted files older than `delete_after_days`
//!      get their bytes purged from object storage and the row deleted for
//!      good.
//!
//! Policy resolution is "most-specific wins":
//!     user > org > system > workspace
//! so a per-user retention can override the workspace default without
//! deleting the workspace rule itself.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone, FromRow)]
struct Policy {
    scope_type: String,
    scope_id:   Option<String>,
    keep_last_n_versions: i32,
    archive_after_days:   i32,
    delete_after_days:    i32,
}

/// Resolved per-file effective policy after merging workspace/system/org/user.
#[derive(Debug, Clone, Default)]
struct Effective {
    keep_last_n_versions: i32,
    archive_after_days:   i32,
    delete_after_days:    i32,
}

#[derive(Debug, Default)]
pub struct RotationStats {
    pub versions_pruned:    i32,
    pub files_archived:     i32,
    pub files_hard_deleted: i32,
    /// Phase 5d — abandoned TUS sessions cleaned up this tick.
    pub tus_reaped:         i32,
}

/// Single run of the rotation engine.  Idempotent so cron + manual triggers
/// can both call it safely.
pub async fn run_once(state: &AppState, triggered_by: Option<&str>) -> anyhow::Result<RotationStats> {
    let run_id = Uuid::now_v7();
    sqlx::query("INSERT INTO rotation_runs (id, triggered_by) VALUES ($1, $2)")
        .bind(run_id).bind(triggered_by)
        .execute(&state.db).await?;

    let res = run_inner(state).await;
    match &res {
        Ok(stats) => {
            sqlx::query(
                "UPDATE rotation_runs SET finished_at = now(),
                    versions_pruned = $2, files_archived = $3, files_hard_deleted = $4
                  WHERE id = $1"
            )
            .bind(run_id)
            .bind(stats.versions_pruned)
            .bind(stats.files_archived)
            .bind(stats.files_hard_deleted)
            .execute(&state.db).await?;
        }
        Err(e) => {
            sqlx::query("UPDATE rotation_runs SET finished_at = now(), error = $2 WHERE id = $1")
                .bind(run_id).bind(format!("{e:#}"))
                .execute(&state.db).await?;
        }
    }
    res
}

async fn run_inner(state: &AppState) -> anyhow::Result<RotationStats> {
    let policies: Vec<Policy> = sqlx::query_as("SELECT scope_type, scope_id, keep_last_n_versions, archive_after_days, delete_after_days FROM rotation_policies")
        .fetch_all(&state.db).await?;

    let workspace = policies.iter().find(|p| p.scope_type == "workspace").cloned();
    let by_system: std::collections::HashMap<String, &Policy> = policies.iter()
        .filter(|p| p.scope_type == "system").filter_map(|p| p.scope_id.as_ref().map(|id| (id.clone(), p))).collect();
    let by_org: std::collections::HashMap<String, &Policy> = policies.iter()
        .filter(|p| p.scope_type == "org").filter_map(|p| p.scope_id.as_ref().map(|id| (id.clone(), p))).collect();
    let by_user: std::collections::HashMap<String, &Policy> = policies.iter()
        .filter(|p| p.scope_type == "user").filter_map(|p| p.scope_id.as_ref().map(|id| (id.clone(), p))).collect();

    let mut stats = RotationStats::default();

    // We pull the file index in a single SELECT so the merge loop stays simple.
    // Live + trashed rows are processed differently (archive vs hard-delete).
    let files: Vec<FileMin> = sqlx::query_as(
        "SELECT id, system_id, org_id, created_by, created_at, deleted_at FROM files"
    ).fetch_all(&state.db).await?;

    for f in files {
        let eff = resolve(&f, workspace.as_ref(), &by_system, &by_org, &by_user);

        // (1) version pruning applies regardless of archive state.
        if eff.keep_last_n_versions > 0 {
            stats.versions_pruned += prune_versions(state, f.id, eff.keep_last_n_versions).await?;
        }

        if f.deleted_at.is_none() {
            // (2) archive: live file older than archive_after_days → soft delete.
            if eff.archive_after_days > 0 {
                let cutoff = Utc::now() - chrono::Duration::days(eff.archive_after_days as i64);
                if f.created_at < cutoff {
                    sqlx::query("UPDATE files SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
                        .bind(f.id).execute(&state.db).await?;
                    stats.files_archived += 1;
                }
            }
        } else {
            // (3) hard delete: soft-deleted file older than delete_after_days
            // → remove from object storage + drop the row.
            if eff.delete_after_days > 0 {
                let cutoff = Utc::now() - chrono::Duration::days(eff.delete_after_days as i64);
                if f.deleted_at.unwrap() < cutoff {
                    if hard_delete_file(state, f.id, cutoff).await.is_ok() {
                        stats.files_hard_deleted += 1;
                    }
                }
            }
        }
    }

    // (4) Reap abandoned TUS resumable upload sessions whose 24h window
    // expired without ever completing.  Without this, a client that
    // disconnects mid-upload leaks a partial blob and a DB row forever.
    stats.tus_reaped += reap_tus_uploads(state).await?;

    Ok(stats)
}

/// Delete `tus_uploads` rows that expired without ever finalising, plus the
/// partial blob each row owns under `tus_root()`.  Returns the number of
/// sessions reaped this tick.
async fn reap_tus_uploads(state: &AppState) -> anyhow::Result<i32> {
    let expired: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, temp_key FROM tus_uploads \
          WHERE completed_at IS NULL AND expires_at < now()"
    ).fetch_all(&state.db).await?;

    let mut n = 0i32;
    for (id, temp_key) in expired {
        // Storage delete is best-effort — if the blob was never written or
        // already cleaned up by hand, we still want to drop the DB row.
        let path = tus_temp_path(&temp_key);
        let _ = tokio::fs::remove_file(&path).await;
        if sqlx::query("DELETE FROM tus_uploads WHERE id = $1 AND completed_at IS NULL")
            .bind(id).execute(&state.db).await.is_ok()
        {
            n += 1;
        }
    }
    Ok(n)
}

/// Mirror of `tus::tus_root()`.  Kept private here so the rotation engine
/// doesn't depend on tus.rs being public-friendly.
fn tus_temp_path(temp_key: &str) -> std::path::PathBuf {
    let root = std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./storage".into());
    std::path::PathBuf::from(root).join(".tus").join(temp_key)
}

#[derive(Debug, FromRow)]
struct FileMin {
    id:          Uuid,
    system_id:   String,
    org_id:      Option<String>,
    created_by:  Option<String>,
    created_at:  DateTime<Utc>,
    deleted_at:  Option<DateTime<Utc>>,
}

fn resolve(
    f: &FileMin,
    workspace: Option<&Policy>,
    by_system: &std::collections::HashMap<String, &Policy>,
    by_org:    &std::collections::HashMap<String, &Policy>,
    by_user:   &std::collections::HashMap<String, &Policy>,
) -> Effective {
    // Walk specificity from most → least specific and take the first non-zero
    // value at each axis.  That way a user can shorten retention without
    // wiping the workspace baseline.
    let chain: Vec<&Policy> = [
        f.created_by.as_ref().and_then(|u| by_user.get(u).copied()),
        f.org_id.as_ref().and_then(|o| by_org.get(o).copied()),
        by_system.get(&f.system_id).copied(),
        workspace,
    ].into_iter().flatten().collect();

    let mut e = Effective::default();
    for p in chain {
        if e.keep_last_n_versions == 0 && p.keep_last_n_versions > 0 { e.keep_last_n_versions = p.keep_last_n_versions; }
        if e.archive_after_days   == 0 && p.archive_after_days   > 0 { e.archive_after_days   = p.archive_after_days; }
        if e.delete_after_days    == 0 && p.delete_after_days    > 0 { e.delete_after_days    = p.delete_after_days; }
    }
    e
}

async fn prune_versions(state: &AppState, file_id: Uuid, keep: i32) -> anyhow::Result<i32> {
    // We keep the current `files` row (= newest) plus the most recent
    // `keep - 1` rows in file_versions.  Everything older gets evicted.
    let to_evict: Vec<(Uuid, String)> = sqlx::query_as(
        r#"SELECT id, object_key FROM file_versions
            WHERE file_id = $1
            ORDER BY version DESC
            OFFSET $2"#
    )
    .bind(file_id).bind(keep.saturating_sub(1) as i64)
    .fetch_all(&state.db).await?;

    let mut n = 0i32;
    for (vid, key) in to_evict {
        // Object removal is best-effort — a missing blob shouldn't block the
        // DB row from being cleaned up.
        let _ = state.storage.delete(&key).await;
        sqlx::query("DELETE FROM file_versions WHERE id = $1")
            .bind(vid).execute(&state.db).await?;
        n += 1;
    }
    Ok(n)
}

/// Hard-delete a file that's already past `delete_after_days`.
///
/// The flow is:
///   1. Fetch the object key + every version key in a single transaction
///      and re-check `deleted_at IS NOT NULL AND deleted_at < cutoff` to
///      protect against a concurrent restore that won the race after the
///      run_inner pre-check.
///   2. Delete the dependent rows (versions, previews, share links) inside
///      the same transaction.  Activity rows are left alone — the FK is
///      `ON DELETE SET NULL`, so the audit history outlives the file.
///   3. Commit the transaction so the row is gone from the DB perspective.
///   4. Only THEN unlink the storage objects.  If unlinks fail we don't
///      roll back the DB delete — the worst case is orphaned bytes, which
///      a re-keying job can sweep, vs. a DB row that has no backing bytes,
///      which is permanently broken.
async fn hard_delete_file(
    state: &AppState,
    file_id: Uuid,
    cutoff: DateTime<Utc>,
) -> anyhow::Result<()> {
    let mut tx = state.db.begin().await?;

    // Recheck the soft-delete window inside the transaction so a concurrent
    // restore (which clears deleted_at) wins the race.  If the file was
    // restored or hard-deleted by someone else we simply do nothing.
    let main: Option<(String,)> = sqlx::query_as(
        "SELECT object_key FROM files \
          WHERE id = $1 AND deleted_at IS NOT NULL AND deleted_at < $2"
    )
    .bind(file_id).bind(cutoff)
    .fetch_optional(&mut *tx).await?;
    let Some((main_key,)) = main else {
        tx.rollback().await.ok();
        return Ok(());
    };

    let versions: Vec<(String,)> = sqlx::query_as(
        "SELECT object_key FROM file_versions WHERE file_id = $1"
    ).bind(file_id).fetch_all(&mut *tx).await?;

    // DB-side cleanup — order matters for FKs, but `activity.file_id` is
    // ON DELETE SET NULL so we deliberately do NOT delete the audit rows.
    sqlx::query("DELETE FROM file_versions WHERE file_id = $1")
        .bind(file_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM file_previews WHERE file_id = $1")
        .bind(file_id).execute(&mut *tx).await.ok();
    sqlx::query("DELETE FROM share_links   WHERE file_id = $1")
        .bind(file_id).execute(&mut *tx).await.ok();
    sqlx::query("DELETE FROM files         WHERE id = $1")
        .bind(file_id).execute(&mut *tx).await?;

    tx.commit().await?;

    // Storage cleanup happens *after* the transaction commits.  Missing
    // blobs are tolerated — a deletion that already happened is a no-op.
    let _ = state.storage.delete(&main_key).await;
    for (k,) in versions {
        let _ = state.storage.delete(&k).await;
    }
    Ok(())
}

/// Spawn a tokio task that runs the rotation engine every `interval`.  Cheap
/// scheduler — no extra deps — and keeps the binary single-process.  The
/// returned `JoinHandle` lets the binary await the worker during graceful
/// shutdown so an in-flight rotation tick isn't orphaned by SIGTERM.
pub fn spawn_worker(state: Arc<AppState>, interval: std::time::Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Initial delay so the worker doesn't fire during migration / boot.
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        loop {
            match run_once(&state, Some("cron")).await {
                Ok(stats) => tracing::info!(
                    "rotation: pruned={} archived={} hard_deleted={} tus_reaped={}",
                    stats.versions_pruned, stats.files_archived,
                    stats.files_hard_deleted, stats.tus_reaped
                ),
                Err(e) => tracing::error!("rotation run failed: {e:#}"),
            }
            tokio::time::sleep(interval).await;
        }
    })
}

// =============================================================================
// Tests for the policy-resolution logic — keeps the merge rules from drifting.
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn p(scope: &str, keep: i32, arch: i32, del: i32) -> Policy {
        Policy {
            scope_type: scope.into(),
            scope_id: None,
            keep_last_n_versions: keep,
            archive_after_days: arch,
            delete_after_days: del,
        }
    }

    #[test]
    fn workspace_only_passes_through() {
        let ws = p("workspace", 5, 0, 30);
        let f  = FileMin {
            id: Uuid::nil(), system_id: "sys_x".into(), org_id: None,
            created_by: None, created_at: Utc::now(), deleted_at: None,
        };
        let e = resolve(&f, Some(&ws), &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(e.keep_last_n_versions, 5);
        assert_eq!(e.archive_after_days,   0);
        assert_eq!(e.delete_after_days,    30);
    }

    #[test]
    fn user_overrides_workspace_for_set_axes_only() {
        let ws       = p("workspace", 5, 0, 30);
        let mut up   = p("user",       2, 0, 0);
        up.scope_id  = Some("usr_a".into());
        let map: HashMap<String, &Policy> = [("usr_a".to_string(), &up)].into_iter().collect();
        let f = FileMin {
            id: Uuid::nil(), system_id: "sys_x".into(), org_id: None,
            created_by: Some("usr_a".into()), created_at: Utc::now(), deleted_at: None,
        };
        let e = resolve(&f, Some(&ws), &HashMap::new(), &HashMap::new(), &map);
        // user shortens retention to 2
        assert_eq!(e.keep_last_n_versions, 2);
        // workspace still provides delete_after_days
        assert_eq!(e.delete_after_days,    30);
    }
}

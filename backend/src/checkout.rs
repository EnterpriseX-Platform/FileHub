//! Document check-out / check-in (MEA TOR 5.3.8.4-5).
//!
//! Check-out locks a file to the current user — others may view but cannot edit
//! or check it out. Check-in releases the lock. Stale locks are auto-released by
//! the rotation worker (see `rotation.rs`, TOR 5.3.8.5).
//!
//! Lock state is three nullable columns on `files` (migration 0017). We keep a
//! denormalised `checked_out_by_name` so the UI can show "checked out by X"
//! without a join. Enforcement of the lock on edits lives in the mutating
//! handlers (e.g. `handlers::upload_version`) via `lock_blocks`.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{ensure_system_access, require_role, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Serialize)]
pub struct LockInfo {
    pub locked: bool,
    pub by_id: Option<String>,
    pub by_name: Option<String>,
    pub at: Option<DateTime<Utc>>,
    /// True when the current caller is the holder (UI shows "Check in").
    pub by_me: bool,
}

type LockRow = (String, Option<String>, Option<String>, Option<DateTime<Utc>>);

/// Load (system_id, checked_out_by, checked_out_by_name, checked_out_at) for a
/// live file, mapping no-access to 404 (don't leak existence).
async fn load(state: &AppState, user: &AuthUser, id: Uuid) -> ApiResult<LockRow> {
    let row: Option<LockRow> = sqlx::query_as(
        "SELECT system_id, checked_out_by, checked_out_by_name, checked_out_at \
         FROM files WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    if ensure_system_access(&state.db, &user.0, &row.0).await.is_err() {
        return Err(ApiError::NotFound);
    }
    Ok(row)
}

fn info(row: &LockRow, me: &str) -> LockInfo {
    let by_id = row.1.clone();
    LockInfo {
        locked: by_id.is_some(),
        by_me: by_id.as_deref() == Some(me),
        by_id,
        by_name: row.2.clone(),
        at: row.3,
    }
}

/// GET /fh/api/files/:id/lock — current lock state.
pub async fn get_lock(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LockInfo>> {
    let row = load(&s, &user, id).await?;
    Ok(Json(info(&row, &user.0.id)))
}

/// POST /fh/api/files/:id/checkout — lock to the current user.
pub async fn checkout(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LockInfo>> {
    require_role(&user.0, &["admin", "editor"])?;
    let row = load(&s, &user, id).await?;
    if let Some(holder) = &row.1 {
        if holder != &user.0.id {
            let who = row.2.clone().unwrap_or_else(|| "another user".into());
            return Err(ApiError::Conflict(format!("already checked out by {who}")));
        }
        // Already held by me — idempotent.
        return Ok(Json(info(&row, &user.0.id)));
    }
    sqlx::query(
        "UPDATE files SET checked_out_by = $2, checked_out_by_name = $3, checked_out_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&user.0.id)
    .bind(&user.0.display_name)
    .execute(&s.db)
    .await?;
    activity(&s, id, &row.0, &user, "checked out").await;
    let row = load(&s, &user, id).await?;
    Ok(Json(info(&row, &user.0.id)))
}

/// POST /fh/api/files/:id/checkin — release the lock (holder or admin only).
pub async fn checkin(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LockInfo>> {
    let row = load(&s, &user, id).await?;
    match &row.1 {
        None => return Ok(Json(info(&row, &user.0.id))), // already free
        Some(holder) if holder != &user.0.id && user.0.role != "admin" => {
            return Err(ApiError::Conflict("only the holder or an admin can check this in".into()));
        }
        _ => {}
    }
    sqlx::query(
        "UPDATE files SET checked_out_by = NULL, checked_out_by_name = NULL, checked_out_at = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(&s.db)
    .await?;
    activity(&s, id, &row.0, &user, "checked in").await;
    let row = load(&s, &user, id).await?;
    Ok(Json(info(&row, &user.0.id)))
}

async fn activity(s: &AppState, id: Uuid, system_id: &str, user: &AuthUser, action: &str) {
    let _ = sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target_type, file_id, system_id, created_at, actor_id)
           VALUES ($1,$2,'slate',$3,'file',$4,$5,now(),$6)"#,
    )
    .bind(Uuid::now_v7())
    .bind(&user.0.display_name)
    .bind(action)
    .bind(id)
    .bind(system_id)
    .bind(&user.0.id)
    .execute(&s.db)
    .await;
}

/// Returns Some(holder_name) if the file is checked out by someone OTHER than
/// `user_id` — callers in mutating handlers use this to block edits with 409.
pub async fn lock_blocks(db: &sqlx::PgPool, file_id: Uuid, user_id: &str) -> Option<String> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT checked_out_by, checked_out_by_name FROM files WHERE id = $1")
            .bind(file_id)
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
    match row {
        Some((Some(holder), name)) if holder != user_id => {
            Some(name.unwrap_or_else(|| "another user".into()))
        }
        _ => None,
    }
}

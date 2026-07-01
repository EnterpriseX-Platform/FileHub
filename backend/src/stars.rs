//! Per-user file favorites ("Starred" in the everyday UI). One row per
//! (user, file) in `file_stars` (migration 0018). Any authenticated user may
//! star anything they can see; access is checked the same way as reads — no
//! access maps to 404 so existence doesn't leak.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{effective_system_ids, ensure_system_access, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::handlers::FILE_COLS;
use crate::models::File;
use crate::state::AppState;

#[derive(Serialize)]
pub struct StarState {
    pub starred: bool,
}

/// Confirm the file exists and the caller may see it; 404 otherwise.
async fn access_or_404(state: &AppState, user: &AuthUser, id: Uuid) -> ApiResult<()> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    if ensure_system_access(&state.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

/// GET /fh/api/files/:id/star — is this file starred by the caller?
pub async fn get_star(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StarState>> {
    access_or_404(&s, &user, id).await?;
    let exists: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM file_stars WHERE user_id = $1 AND file_id = $2")
            .bind(&user.0.id)
            .bind(id)
            .fetch_optional(&s.db)
            .await?;
    Ok(Json(StarState { starred: exists.is_some() }))
}

/// PUT /fh/api/files/:id/star — star (idempotent).
pub async fn add_star(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StarState>> {
    access_or_404(&s, &user, id).await?;
    sqlx::query("INSERT INTO file_stars (user_id, file_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(&user.0.id)
        .bind(id)
        .execute(&s.db)
        .await?;
    Ok(Json(StarState { starred: true }))
}

/// DELETE /fh/api/files/:id/star — unstar.
pub async fn remove_star(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StarState>> {
    access_or_404(&s, &user, id).await?;
    sqlx::query("DELETE FROM file_stars WHERE user_id = $1 AND file_id = $2")
        .bind(&user.0.id)
        .bind(id)
        .execute(&s.db)
        .await?;
    Ok(Json(StarState { starred: false }))
}

/// GET /fh/api/starred — the caller's starred files, permission-scoped, most
/// recently modified first.
pub async fn list_starred(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<File>>> {
    let scope = effective_system_ids(&s.db, &user.0).await?;
    // IN-subquery keeps FILE_COLS (bare column names) unambiguous — only `files`
    // is in the FROM clause.
    let mut sql = format!(
        "SELECT {FILE_COLS} FROM files f \
         WHERE f.id IN (SELECT file_id FROM file_stars WHERE user_id = $1) \
           AND f.deleted_at IS NULL"
    );
    if scope.is_some() {
        sql.push_str(" AND f.system_id = ANY($2)");
    }
    sql.push_str(" ORDER BY f.modified_at DESC");

    let mut q = sqlx::query_as::<_, File>(&sql).bind(&user.0.id);
    if let Some(ref sc) = scope {
        q = q.bind(sc);
    }
    Ok(Json(q.fetch_all(&s.db).await?))
}

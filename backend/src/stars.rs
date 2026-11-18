//! Starred baskets and virtual baskets — what the side nav shows first.

use std::sync::Arc;

use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::Serialize;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Serialize, sqlx::FromRow)]
pub struct Star { pub kind: String, pub id: String }

pub async fn list(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Vec<Star>>> {
    Ok(Json(sqlx::query_as("SELECT kind, ref_id AS id FROM user_stars WHERE user_id = $1 ORDER BY created_at")
        .bind(&user.0.id).fetch_all(&s.db).await?))
}

pub async fn add(State(s): State<Arc<AppState>>, user: AuthUser, Path((kind, id)): Path<(String, String)>) -> ApiResult<StatusCode> {
    // Only things the person can open.
    let ok: bool = match kind.as_str() {
        "basket" => {
            crate::auth::ensure_system_access(&s.db, &user.0, &id).await?;
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM systems WHERE id = $1 AND deleted_at IS NULL)").bind(&id).fetch_one(&s.db).await?
        }
        "vbasket" => sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM vbaskets WHERE id = $1 AND deleted_at IS NULL)").bind(&id).fetch_one(&s.db).await?,
        _ => return Err(ApiError::BadRequest("kind must be basket or vbasket".into())),
    };
    if !ok { return Err(ApiError::NotFound); }
    sqlx::query("INSERT INTO user_stars (user_id, kind, ref_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(&user.0.id).bind(&kind).bind(&id).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove(State(s): State<Arc<AppState>>, user: AuthUser, Path((kind, id)): Path<(String, String)>) -> ApiResult<StatusCode> {
    sqlx::query("DELETE FROM user_stars WHERE user_id = $1 AND kind = $2 AND ref_id = $3")
        .bind(&user.0.id).bind(&kind).bind(&id).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

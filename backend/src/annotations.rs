//! Document annotations (TOR ANNEX-3): notes, highlights, and signature
//! stamps placed on the rendered page by the pdf.js viewer. Coordinates are
//! normalized to the page (0..1), so they are render-scale independent.
//! Any role that can SEE the file can annotate (mirrors comments); deleting
//! is limited to the author or an admin.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{ensure_system_access, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Serialize, sqlx::FromRow)]
pub struct Annotation {
    pub id: Uuid,
    pub file_id: Uuid,
    pub page: i32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub kind: String,
    pub body: Option<String>,
    pub signature_id: Option<Uuid>,
    /// The stamp's mark image (data URL), joined in so the viewer renders
    /// without one request per stamp.
    pub signature_image: Option<String>,
    pub created_by: Option<String>,
    pub author: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct NewAnnotation {
    pub page: i32,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
    pub kind: String,
    pub body: Option<String>,
    pub signature_id: Option<Uuid>,
}

/// Confirm the file exists and the caller may see it; 404 otherwise (reads
/// must not leak existence).
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

const SELECT: &str = "SELECT a.id, a.file_id, a.page, a.x, a.y, a.w, a.h, a.kind, a.body, \
                             a.signature_id, sg.image AS signature_image, \
                             a.created_by, u.display_name AS author, a.created_at \
                      FROM annotations a \
                      LEFT JOIN users u ON u.id = a.created_by \
                      LEFT JOIN signatures sg ON sg.id = a.signature_id";

/// GET /fh/api/files/:id/annotations
pub async fn list(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Annotation>>> {
    access_or_404(&s, &user, file_id).await?;
    let rows = sqlx::query_as::<_, Annotation>(&format!(
        "{SELECT} WHERE a.file_id = $1 ORDER BY a.page, a.created_at"
    ))
    .bind(file_id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(rows))
}

/// POST /fh/api/files/:id/annotations
pub async fn create(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
    Json(b): Json<NewAnnotation>,
) -> ApiResult<Json<Annotation>> {
    access_or_404(&s, &user, file_id).await?;
    if !matches!(b.kind.as_str(), "note" | "highlight" | "stamp") {
        return Err(ApiError::BadRequest("kind must be note, highlight, or stamp".into()));
    }
    if b.kind == "note" && b.body.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(ApiError::BadRequest("a note needs text".into()));
    }
    if b.kind == "stamp" && b.signature_id.is_none() {
        return Err(ApiError::BadRequest("a stamp needs signature_id".into()));
    }
    // A stamp must reference a mark from the CALLER's own library.
    if let Some(sig) = b.signature_id {
        let owned: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM signatures WHERE id = $1 AND user_id = $2")
                .bind(sig)
                .bind(&user.0.id)
                .fetch_optional(&s.db)
                .await?;
        if owned.is_none() {
            return Err(ApiError::BadRequest("signature_id is not in your signature library".into()));
        }
    }

    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO annotations (id, file_id, page, x, y, w, h, kind, body, signature_id, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(file_id)
    .bind(b.page.max(1))
    .bind(b.x.clamp(0.0, 1.0))
    .bind(b.y.clamp(0.0, 1.0))
    .bind(b.w.clamp(0.0, 1.0))
    .bind(b.h.clamp(0.0, 1.0))
    .bind(&b.kind)
    .bind(&b.body)
    .bind(b.signature_id)
    .bind(&user.0.id)
    .execute(&s.db)
    .await?;

    let row = sqlx::query_as::<_, Annotation>(&format!("{SELECT} WHERE a.id = $1"))
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    Ok(Json(row))
}

/// DELETE /fh/api/annotations/:id — author or admin only.
pub async fn delete(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    let row: Option<(Uuid, Option<String>)> =
        sqlx::query_as("SELECT file_id, created_by FROM annotations WHERE id = $1")
            .bind(id)
            .fetch_optional(&s.db)
            .await?;
    let (file_id, created_by) = row.ok_or(ApiError::NotFound)?;
    access_or_404(&s, &user, file_id).await?;
    if created_by.as_deref() != Some(user.0.id.as_str()) && user.0.role != "admin" {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("DELETE FROM annotations WHERE id = $1")
        .bind(id)
        .execute(&s.db)
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

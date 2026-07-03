//! Configurable workflow engine — reusable no-code templates and send-back
//! (TOR Annex A: ระบบงานเอกสารตามกระบวนงาน). The running-workflow model +
//! sequential/parallel routing + turn-guard live in `p1.rs`; this module adds:
//!   * Workflow templates — an admin defines an ordered list of steps once and
//!     reuses it (`p1::start_workflow` expands a template).
//!   * Send-back — return a workflow to an earlier step for rework.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::auth::{require_role, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Serialize, sqlx::FromRow)]
pub struct WorkflowTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub order_mode: String,
    pub steps: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct NewTemplate {
    pub name: String,
    pub description: Option<String>,
    pub order_mode: Option<String>,
    /// [{ name?, reviewer_id }]
    pub steps: Vec<Value>,
}

/// GET /fh/api/workflow-templates — reusable flow templates.
pub async fn list_templates(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<WorkflowTemplate>>> {
    // Templates are only ever used by admins (Settings › Workflows) and
    // editors (the "Start workflow" panel). Gate reads to editor+ so a viewer
    // can't enumerate approval routes + reviewer identities they never need —
    // matching create/delete, which are already editor+.
    require_role(&user.0, &["admin", "editor"])?;
    Ok(Json(sqlx::query_as::<_, WorkflowTemplate>(
        "SELECT id, name, description, order_mode, steps, created_at FROM workflow_templates ORDER BY created_at DESC",
    )
    .fetch_all(&s.db)
    .await?))
}

/// POST /fh/api/workflow-templates — define a reusable template (editor+).
pub async fn create_template(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(b): Json<NewTemplate>,
) -> ApiResult<Json<WorkflowTemplate>> {
    require_role(&user.0, &["admin", "editor"])?;
    if b.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if b.steps.is_empty() {
        return Err(ApiError::BadRequest("a template needs at least one step".into()));
    }
    let order_mode = match b.order_mode.as_deref() {
        Some("parallel") => "parallel",
        _ => "sequential",
    };
    let id = format!("wft_{}", Uuid::now_v7().simple());
    sqlx::query(
        "INSERT INTO workflow_templates (id, name, description, order_mode, steps, created_by) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(&id)
    .bind(b.name.trim())
    .bind(&b.description)
    .bind(order_mode)
    .bind(Value::Array(b.steps))
    .bind(&user.0.id)
    .execute(&s.db)
    .await?;
    Ok(Json(sqlx::query_as::<_, WorkflowTemplate>(
        "SELECT id, name, description, order_mode, steps, created_at FROM workflow_templates WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(&s.db)
    .await?))
}

/// DELETE /fh/api/workflow-templates/:id (editor+).
pub async fn delete_template(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    require_role(&user.0, &["admin", "editor"])?;
    sqlx::query("DELETE FROM workflow_templates WHERE id = $1")
        .bind(&id)
        .execute(&s.db)
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// PATCH /fh/api/workflow-templates/:id — edit a template's name / mode / steps
/// (editor+). Existing running workflows aren't touched (they hold their own
/// step rows); only future starts of this template use the new route.
pub async fn update_template(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(b): Json<NewTemplate>,
) -> ApiResult<Json<WorkflowTemplate>> {
    require_role(&user.0, &["admin", "editor"])?;
    if b.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if b.steps.is_empty() {
        return Err(ApiError::BadRequest("a template needs at least one step".into()));
    }
    let order_mode = match b.order_mode.as_deref() {
        Some("parallel") => "parallel",
        _ => "sequential",
    };
    let n = sqlx::query(
        "UPDATE workflow_templates SET name = $2, description = $3, order_mode = $4, steps = $5 WHERE id = $1",
    )
    .bind(&id)
    .bind(b.name.trim())
    .bind(&b.description)
    .bind(order_mode)
    .bind(Value::Array(b.steps))
    .execute(&s.db)
    .await?;
    if n.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(sqlx::query_as::<_, WorkflowTemplate>(
        "SELECT id, name, description, order_mode, steps, created_at FROM workflow_templates WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(&s.db)
    .await?))
}

/// POST /fh/api/workflow-steps/:id/send-back — reopen this step and every step
/// after it, returning the workflow to this reviewer for rework (editor+).
pub async fn send_back(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(step_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    require_role(&user.0, &["admin", "editor"])?;
    let row: Option<(Uuid, i32, Option<String>)> = sqlx::query_as(
        "SELECT workflow_id, sequence, reviewer_id FROM workflow_steps WHERE id = $1",
    )
    .bind(step_id)
    .fetch_optional(&s.db)
    .await?;
    let (workflow_id, sequence, reviewer_id) = row.ok_or(ApiError::NotFound)?;

    // Gate on the underlying file's access.
    let file_row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT f.id, f.system_id FROM file_workflows wf JOIN files f ON f.id = wf.file_id WHERE wf.id = $1",
    )
    .bind(workflow_id)
    .fetch_optional(&s.db)
    .await?;
    let (file_id, system_id) = file_row.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;

    // Reopen this step and all later ones.
    sqlx::query(
        "UPDATE workflow_steps SET decision = 'pending', note = NULL, decided_at = NULL \
         WHERE workflow_id = $1 AND sequence >= $2",
    )
    .bind(workflow_id)
    .bind(sequence)
    .execute(&s.db)
    .await?;
    sqlx::query("UPDATE file_workflows SET state = 'Review' WHERE id = $1")
        .bind(workflow_id)
        .execute(&s.db)
        .await?;
    sqlx::query("UPDATE files SET status = 'Review', modified_at = now() WHERE id = $1")
        .bind(file_id)
        .execute(&s.db)
        .await?;

    if let Some(rid) = reviewer_id {
        crate::p1::notify_reviewer(&s.db, &rid, &Some("A document was sent back for your review.".into()), file_id).await;
    }
    Ok(Json(serde_json::json!({ "workflow_id": workflow_id, "reopened_from": sequence, "state": "Review" })))
}

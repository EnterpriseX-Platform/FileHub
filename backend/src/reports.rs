//! Document status report + CSV exports (MEA TOR 5.3.1.16, 5.3.7.6).
//!
//! Status report maps FileHub's lifecycle onto the four TOR states:
//!   Active     — live files (deleted_at IS NULL)
//!   Inactive   — archived/trashed but still retained (deleted_at IS NOT NULL)
//!   Retention  — retained past the purge threshold, awaiting hard-delete
//!   Delete     — permanently purged (counted from rotation_runs history)
//! Everything is permission-scoped via effective_system_ids.
//!
//! Exports are CSV with a UTF-8 BOM so Excel opens Thai correctly (the TOR's
//! "CSV และ excel"); a native .xlsx writer is a later add.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{effective_system_ids, require_role, AuthUser};
use crate::error::ApiResult;
use crate::state::AppState;

const RETENTION_DAYS: i64 = 30;

#[derive(Serialize)]
pub struct StatusItem {
    pub id: Uuid,
    pub name: String,
    pub system_id: String,
    pub status: String,
    pub state: String,
    pub modified_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct StatusReport {
    pub active: i64,
    pub inactive: i64,
    pub retention: i64,
    pub deleted: i64,
    pub items: Vec<StatusItem>,
}

fn state_of(deleted_at: Option<DateTime<Utc>>, cutoff: DateTime<Utc>) -> &'static str {
    match deleted_at {
        None => "active",
        Some(t) if t < cutoff => "retention",
        Some(_) => "inactive",
    }
}

async fn count(db: &sqlx::PgPool, where_extra: &str, scope: &Option<Vec<String>>) -> i64 {
    let mut sql = format!("SELECT count(*) FROM files WHERE {where_extra}");
    if scope.is_some() {
        sql.push_str(" AND system_id = ANY($1)");
    }
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    if let Some(s) = scope {
        q = q.bind(s);
    }
    q.fetch_one(db).await.unwrap_or(0)
}

async fn build_status(s: &AppState, user: &AuthUser) -> ApiResult<StatusReport> {
    let scope = effective_system_ids(&s.db, &user.0).await?;
    let cutoff = Utc::now() - Duration::days(RETENTION_DAYS);

    let active = count(&s.db, "deleted_at IS NULL", &scope).await;
    let inactive = count(&s.db, "deleted_at IS NOT NULL", &scope).await;
    let retention = count(
        &s.db,
        &format!("deleted_at IS NOT NULL AND deleted_at < now() - interval '{RETENTION_DAYS} days'"),
        &scope,
    )
    .await;
    // Purged files no longer exist in `files`; the count comes from the worker's run history.
    let deleted: i64 = sqlx::query_scalar("SELECT coalesce(sum(files_hard_deleted),0)::bigint FROM rotation_runs")
        .fetch_one(&s.db)
        .await
        .unwrap_or(0);

    let mut sql = String::from(
        "SELECT id, name, system_id, status, modified_at, deleted_at FROM files",
    );
    if scope.is_some() {
        sql.push_str(" WHERE system_id = ANY($1)");
    }
    sql.push_str(" ORDER BY modified_at DESC LIMIT 200");
    let mut q = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>, Option<DateTime<Utc>>)>(&sql);
    if let Some(ref sc) = scope {
        q = q.bind(sc);
    }
    let rows = q.fetch_all(&s.db).await?;
    let items = rows
        .into_iter()
        .map(|(id, name, system_id, status, modified_at, deleted_at)| StatusItem {
            id,
            name,
            system_id,
            status,
            state: state_of(deleted_at, cutoff).to_string(),
            modified_at,
            deleted_at,
        })
        .collect();

    Ok(StatusReport { active, inactive, retention, deleted, items })
}

/// GET /fh/api/reports/status
pub async fn status_report(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<StatusReport>> {
    Ok(Json(build_status(&s, &user).await?))
}

/// GET /fh/api/reports/status.csv
pub async fn status_csv(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Response> {
    let rep = build_status(&s, &user).await?;
    let mut body = String::from("name,system,status,state,modified_at,deleted_at\n");
    for it in &rep.items {
        body.push_str(&format!(
            "{},{},{},{},{},{}\n",
            cell(&it.name),
            cell(&it.system_id),
            cell(&it.status),
            cell(&it.state),
            cell(&it.modified_at.to_rfc3339()),
            cell(&it.deleted_at.map(|d| d.to_rfc3339()).unwrap_or_default()),
        ));
    }
    Ok(csv("document-status.csv", body))
}

#[derive(Deserialize)]
pub struct AuditQ {
    pub from: Option<String>,
    pub to: Option<String>,
    pub user: Option<String>,
}

/// GET /fh/api/activity/export.csv?from=&to=&user= — User Audit Log export
/// (TOR 5.3.7.6): searchable by date range + user, exported as Excel-readable CSV.
pub async fn audit_csv(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<AuditQ>,
) -> ApiResult<Response> {
    let scope = effective_system_ids(&s.db, &user.0).await?;
    let mut sql = String::from(
        "SELECT created_at, actor, action, coalesce(target,''), coalesce(system_id,'') \
         FROM activity \
         WHERE ($1::timestamptz IS NULL OR created_at >= $1::timestamptz) \
           AND ($2::timestamptz IS NULL OR created_at <= $2::timestamptz) \
           AND ($3::text IS NULL OR actor ILIKE $3)",
    );
    if scope.is_some() {
        sql.push_str(" AND (system_id IS NULL OR system_id = ANY($4))");
    }
    sql.push_str(" ORDER BY created_at DESC LIMIT 5000");

    let user_like = q.user.as_ref().map(|u| format!("%{u}%"));
    let mut query = sqlx::query_as::<_, (DateTime<Utc>, String, String, String, String)>(&sql)
        .bind(&q.from)
        .bind(&q.to)
        .bind(&user_like);
    if let Some(ref sc) = scope {
        query = query.bind(sc);
    }
    let rows = query.fetch_all(&s.db).await?;

    let mut body = String::from("timestamp,user,action,target,system\n");
    for (ts, actor, action, target, system_id) in rows {
        body.push_str(&format!(
            "{},{},{},{},{}\n",
            cell(&ts.to_rfc3339()),
            cell(&actor),
            cell(&action),
            cell(&target),
            cell(&system_id),
        ));
    }
    Ok(csv("audit-log.csv", body))
}

// ---- AI usage / metering report (admin-only) -------------------------------

#[derive(Serialize)]
pub struct AiUsageBucket {
    pub key: String,
    pub ops: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Serialize)]
pub struct AiUsageReport {
    pub total_ops: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub by_op: Vec<AiUsageBucket>,
    pub by_model: Vec<AiUsageBucket>,
}

async fn usage_grouped(db: &sqlx::PgPool, col: &str, q: &AuditQ) -> Vec<AiUsageBucket> {
    // `col` is a fixed identifier ("op" | "model"), never user input — safe to format.
    let sql = format!(
        "SELECT coalesce({col}, '') AS k, count(*)::bigint, \
                coalesce(sum(input_tokens),0)::bigint, coalesce(sum(output_tokens),0)::bigint \
         FROM ai_usage \
         WHERE ($1::timestamptz IS NULL OR created_at >= $1::timestamptz) \
           AND ($2::timestamptz IS NULL OR created_at <= $2::timestamptz) \
         GROUP BY k ORDER BY 2 DESC"
    );
    sqlx::query_as::<_, (String, i64, i64, i64)>(&sql)
        .bind(&q.from)
        .bind(&q.to)
        .fetch_all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(key, ops, input_tokens, output_tokens)| AiUsageBucket { key, ops, input_tokens, output_tokens })
        .collect()
}

/// GET /fh/api/reports/ai-usage?from=&to= — token/op metering for billing
/// (the producer side lives in `ai::record_usage`). **Admin-only** — usage is a
/// workspace-wide billing concern, not per-system.
pub async fn ai_usage_report(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<AuditQ>,
) -> ApiResult<Json<AiUsageReport>> {
    require_role(&user.0, &["admin"])?;
    let by_op = usage_grouped(&s.db, "op", &q).await;
    let by_model = usage_grouped(&s.db, "model", &q).await;
    let total_ops = by_op.iter().map(|b| b.ops).sum();
    let total_input_tokens = by_op.iter().map(|b| b.input_tokens).sum();
    let total_output_tokens = by_op.iter().map(|b| b.output_tokens).sum();
    Ok(Json(AiUsageReport { total_ops, total_input_tokens, total_output_tokens, by_op, by_model }))
}

fn cell(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn csv(filename: &str, body: String) -> Response {
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, "text/csv; charset=utf-8".parse().unwrap());
    h.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{filename}\"").parse().unwrap(),
    );
    // BOM so Excel detects UTF-8 and renders Thai correctly.
    (StatusCode::OK, h, format!("\u{FEFF}{body}")).into_response()
}

// ---------------------------------------------------------------------------
// Requests report — a workspace-level summary of form/document requests by
// status and by type. Admin/editor only (console-level metric).
// ---------------------------------------------------------------------------
#[derive(Serialize, sqlx::FromRow)]
pub struct KindCount {
    pub label: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct RequestsReport {
    pub total: i64,
    pub in_review: i64,
    pub approved: i64,
    pub rejected: i64,
    pub withdrawn: i64,
    pub by_kind: Vec<KindCount>,
}

/// GET /fh/api/reports/requests — counts by status + by request type.
pub async fn requests_report(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<RequestsReport>> {
    require_role(&user.0, &["admin", "editor"])?;
    // A request's status = withdrawn (cancelled_at) else the anchor workflow's
    // state; no workflow / Draft / Review all read as "in review".
    let (total, withdrawn, approved, rejected, in_review): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*)::bigint, \
                count(*) FILTER (WHERE r.cancelled_at IS NOT NULL)::bigint, \
                count(*) FILTER (WHERE r.cancelled_at IS NULL AND wf.state = 'Approved')::bigint, \
                count(*) FILTER (WHERE r.cancelled_at IS NULL AND wf.state = 'Rejected')::bigint, \
                count(*) FILTER (WHERE r.cancelled_at IS NULL AND (wf.state IS NULL OR wf.state IN ('Review','Draft')))::bigint \
           FROM requests r \
           LEFT JOIN file_workflows wf ON wf.file_id = r.file_id",
    )
    .fetch_one(&s.db)
    .await?;

    let by_kind: Vec<KindCount> = sqlx::query_as(
        "SELECT COALESCE(rf.name_en, r.kind) AS label, count(*)::bigint AS count \
           FROM requests r LEFT JOIN request_forms rf ON rf.id = r.kind \
          GROUP BY COALESCE(rf.name_en, r.kind) ORDER BY count(*) DESC",
    )
    .fetch_all(&s.db)
    .await?;

    Ok(Json(RequestsReport { total, in_review, approved, rejected, withdrawn, by_kind }))
}

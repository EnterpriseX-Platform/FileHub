//! Requests — form/document submissions that flow through org approvals.
//!
//! A *request* is a thin overlay on the existing approval engine:
//!   * `POST /api/requests/intake`  — AI turns a plain-language message into a
//!     structured draft (kind + fields + title + summary) and a suggested
//!     approval route. No persistence — the client reviews before submitting.
//!   * `POST /api/requests`         — persists the request, generates or accepts
//!     an anchor document, and starts a `file_workflows` workflow on it (via
//!     `p1::start_workflow_core`) so approvals/notifications/send-back/audit are
//!     all reused. Any signed-in user may submit; approvals stay editor+.
//!   * `GET  /api/requests?box=…`   — the caller's submissions (`mine`) or the
//!     requests awaiting their decision (`inbox`).
//!   * `GET  /api/requests/:id`     — full detail (form + anchor + workflow).
//!   * `GET  /api/request-forms`    — the static form schemas (for the manual
//!     form + to tell the model which field keys to target).
//!
//! Decisions reuse `POST /api/workflow-steps/:id/decision` + `/send-back`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::ai;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::p1::{Workflow, WorkflowStep};
use crate::state::AppState;

/// The shared area every request's anchor document lives in, so any assigned
/// reviewer can read it. A personal drive would 403 the reviewers.
const REQ_SYSTEM: &str = "sys_requests";

// ---------------------------------------------------------------------------
// Form schemas — static. One per request kind. Served to the frontend (manual
// form) and injected into the intake prompt so the model targets real keys.
// ---------------------------------------------------------------------------
#[derive(Serialize)]
pub struct FormField {
    pub key: &'static str,
    pub label_en: &'static str,
    pub label_th: &'static str,
    pub kind: &'static str, // text | textarea | number | money | date
    pub required: bool,
}
#[derive(Serialize)]
pub struct FormSchema {
    pub kind: &'static str,
    pub name_en: &'static str,
    pub name_th: &'static str,
    pub fields: Vec<FormField>,
}

fn f(key: &'static str, en: &'static str, th: &'static str, kind: &'static str, required: bool) -> FormField {
    FormField { key, label_en: en, label_th: th, kind, required }
}

pub fn schemas() -> Vec<FormSchema> {
    vec![
        FormSchema { kind: "expense", name_en: "Expense request", name_th: "เบิกค่าใช้จ่าย", fields: vec![
            f("category", "Category", "หมวด", "text", true),
            f("amount", "Amount (THB)", "จำนวนเงิน (บาท)", "money", true),
            f("date", "Date", "วันที่", "date", false),
            f("cost_center", "Cost center", "ศูนย์ต้นทุน", "text", false),
            f("purpose", "Purpose", "วัตถุประสงค์", "textarea", true),
        ]},
        FormSchema { kind: "it", name_en: "IT / service request", name_th: "คำขอไอที", fields: vec![
            f("item", "Item / service", "รายการ", "text", true),
            f("priority", "Priority", "ความเร่งด่วน", "text", false),
            f("reason", "Reason", "เหตุผล", "textarea", true),
        ]},
        FormSchema { kind: "document", name_en: "Document approval", name_th: "อนุมัติเอกสาร", fields: vec![
            f("document", "Document", "เอกสาร", "text", true),
            f("deadline", "Deadline", "กำหนดส่ง", "date", false),
            f("note", "Note", "หมายเหตุ", "textarea", false),
        ]},
        FormSchema { kind: "leave", name_en: "Leave request", name_th: "คำขอลา", fields: vec![
            f("leave_type", "Leave type", "ประเภทการลา", "text", true),
            f("from", "From", "ตั้งแต่", "date", true),
            f("to", "To", "ถึง", "date", true),
            f("reason", "Reason", "เหตุผล", "textarea", false),
        ]},
    ]
}

pub async fn list_forms(_user: AuthUser) -> Json<Vec<FormSchema>> {
    Json(schemas())
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "it" => "IT / service request",
        "document" => "Document approval",
        "leave" => "Leave request",
        _ => "Expense request",
    }
}

/// Coerce whatever the model (or client) sent into one of the four known kinds.
fn normalize_kind(k: Option<&str>) -> String {
    let v = k.map(|s| s.trim().to_lowercase()).unwrap_or_default();
    match v.as_str() {
        "it" | "service" | "it/service" | "it_service" | "equipment" => "it",
        "document" | "doc" | "approval" | "document_approval" | "sign" => "document",
        "leave" | "vacation" | "time_off" => "leave",
        _ => "expense",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Routing policy — kind (+ amount) → the sequential reviewer chain. Targets
// seed reviewer users. The requester may override this before submitting
// (see CreateReq.reviewers), so this is a smart default, not a hard rule.
// ---------------------------------------------------------------------------
type Step = (&'static str, &'static str); // (reviewer_id, step label)

fn policy_route(kind: &str, amount: Option<f64>) -> (String, Vec<Step>) {
    let steps: Vec<Step> = match kind {
        "expense" => {
            let mut v = vec![("usr_krit", "Manager"), ("usr_pat", "Finance")];
            if amount.unwrap_or(0.0) > 5000.0 {
                v.push(("usr_sarah", "Director"));
            }
            v
        }
        "it" => vec![("usr_pat", "IT"), ("usr_krit", "Asset owner")],
        "document" => vec![("usr_wisanu", "Legal"), ("usr_sarah", "Director")],
        "leave" => vec![("usr_krit", "Manager")],
        _ => vec![("usr_krit", "Manager")],
    };
    ("sequential".into(), steps)
}

#[derive(Serialize)]
pub struct RouteStep {
    pub reviewer_id: String,
    pub reviewer_name: String,
    pub step_name: String,
}

async fn resolve_route(db: &sqlx::PgPool, steps: &[Step]) -> Vec<RouteStep> {
    let mut out = Vec::with_capacity(steps.len());
    for (id, step) in steps {
        let name: Option<(String,)> = sqlx::query_as("SELECT display_name FROM users WHERE id = $1")
            .bind(id).fetch_optional(db).await.ok().flatten();
        out.push(RouteStep {
            reviewer_id: (*id).to_string(),
            reviewer_name: name.map(|n| n.0).unwrap_or_else(|| "Reviewer".into()),
            step_name: (*step).to_string(),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Intake — plain language → structured draft. AI only; the client falls back
// to the manual form when this is unavailable.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
pub struct IntakeReq {
    pub message: String,
}

#[derive(Serialize)]
pub struct IntakeResult {
    pub kind: String,
    pub title: String,
    pub fields: Value,
    pub amount: Option<f64>,
    pub ai_summary: String,
    pub order_mode: String,
    pub route: Vec<RouteStep>,
}

/// Pull the first {...} block out of a possibly-chatty model response.
fn json_slice(raw: &str) -> &str {
    match (raw.find('{'), raw.rfind('}')) {
        (Some(a), Some(b)) if b > a => &raw[a..=b],
        _ => raw.trim(),
    }
}

pub async fn intake(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<IntakeReq>,
) -> ApiResult<Json<IntakeResult>> {
    if !s.ai.enabled() {
        return Err(ApiError::BadRequest("AI is disabled (AI_ENABLED=false)".into()));
    }
    if req.message.trim().is_empty() {
        return Err(ApiError::BadRequest("message is required".into()));
    }

    // Describe the catalog so the model targets real field keys.
    let mut catalog = String::new();
    for sc in schemas() {
        let keys: Vec<&str> = sc.fields.iter().map(|fl| fl.key).collect();
        catalog.push_str(&format!("- {} ({}): fields = [{}]\n", sc.kind, sc.name_en, keys.join(", ")));
    }

    let system = "You convert a plain-language staff request into a structured form. \
        Respond ONLY with a single compact JSON object — no prose, no code fences.";
    let prompt = format!(
        "Request types and their field keys:\n{catalog}\n\
         User request:\n\"{}\"\n\n\
         Return JSON with keys: \
         \"kind\" (one of expense, it, document, leave), \
         \"title\" (a short human title, <= 8 words), \
         \"amount\" (number in THB or null; only for expense), \
         \"fields\" (object keyed by that kind's field keys, values from the message; omit unknowns), \
         \"summary\" (one sentence written for the approver, stating the ask and any policy-relevant fact).",
        req.message
    );

    let (raw, usage) = s.ai.chat(system, &prompt).await?;
    ai::record_usage(&s.db, Some(&user.0.id), Some(REQ_SYSTEM), "request_intake", s.ai.chat_model(), &usage).await;

    #[derive(Deserialize, Default)]
    struct Ext {
        kind: Option<String>,
        title: Option<String>,
        amount: Option<f64>,
        fields: Option<Value>,
        summary: Option<String>,
    }
    let ext: Ext = serde_json::from_str(json_slice(&raw)).unwrap_or_default();

    let kind = normalize_kind(ext.kind.as_deref());
    let (order_mode, steps) = policy_route(&kind, ext.amount);
    let route = resolve_route(&s.db, &steps).await;

    Ok(Json(IntakeResult {
        title: ext.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| kind_label(&kind).to_string()),
        kind,
        fields: ext.fields.unwrap_or_else(|| json!({})),
        amount: ext.amount,
        ai_summary: ext.summary.unwrap_or_default(),
        order_mode,
        route,
    }))
}

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
pub struct RouteStepIn {
    pub reviewer_id: String,
    pub step_name: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateReq {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub form_data: Value,
    pub amount: Option<f64>,
    /// An already-uploaded anchor document in the Requests area. If absent, the
    /// backend generates a summary document from the form.
    pub file_id: Option<Uuid>,
    /// Caller-edited approval route. If absent/empty, the policy route is used.
    pub reviewers: Option<Vec<RouteStepIn>>,
    pub order_mode: Option<String>,
    pub ai_summary: Option<String>,
}

fn render_request_doc(title: &str, kind: &str, form: &Value) -> String {
    let mut out = format!("# {title}\n\nRequest type: {}\n\n", kind_label(kind));
    if let Some(obj) = form.as_object() {
        for (k, v) in obj {
            let val = match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            if !val.is_empty() {
                out.push_str(&format!("- {k}: {val}\n"));
            }
        }
    }
    out
}

pub async fn create(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<CreateReq>,
) -> ApiResult<Json<RequestDetail>> {
    // Any signed-in user may submit their own request (viewers included). The
    // approval steps remain editor+ (enforced by the workflow decision handler).
    if req.title.trim().is_empty() {
        return Err(ApiError::BadRequest("title is required".into()));
    }
    let kind = normalize_kind(Some(&req.kind));

    // Resolve the anchor document.
    let file_id: Uuid = if let Some(fid) = req.file_id {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
                .bind(fid).fetch_optional(&s.db).await?;
        let (sys,) = row.ok_or_else(|| ApiError::BadRequest("attachment not found".into()))?;
        if sys != REQ_SYSTEM {
            return Err(ApiError::BadRequest("attachment must be uploaded to the Requests area".into()));
        }
        fid
    } else {
        let body = render_request_doc(&req.title, &kind, &req.form_data);
        let name = format!("{}.md", req.title.trim());
        let file = crate::handlers::create_file_from_bytes(
            &s, Some(&user.0.id), name, REQ_SYSTEM.into(), None,
            Some(user.0.display_name.clone()), body.into_bytes().into(), Some("text/markdown".into()),
        ).await?;
        file.id
    };

    let req_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO requests (id, kind, title, form_data, amount, file_id, system_id, ai_summary, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
    )
    .bind(req_id).bind(&kind).bind(&req.title).bind(&req.form_data).bind(req.amount)
    .bind(file_id).bind(REQ_SYSTEM).bind(&req.ai_summary).bind(&user.0.id)
    .execute(&s.db).await?;

    // Route: caller override if provided, else policy.
    let (order_mode, steps): (String, Vec<(String, Option<String>)>) =
        match req.reviewers.filter(|v| !v.is_empty()) {
            Some(rv) => {
                let om = req.order_mode.clone().unwrap_or_else(|| "sequential".into());
                (om, rv.into_iter().map(|r| (r.reviewer_id, r.step_name)).collect())
            }
            None => {
                let (om, st) = policy_route(&kind, req.amount);
                (om, st.into_iter().map(|(id, name)| (id.to_string(), Some(name.to_string()))).collect())
            }
        };

    crate::p1::start_workflow_core(
        &s.db, file_id, &user.0.id, &Some(req.title.clone()), &order_mode, &steps, None,
    ).await?;

    let _ = sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'indigo','submitted a request',$3,'request',$4,$5,NULL,now(),$6)"#,
    )
    .bind(Uuid::now_v7()).bind(&user.0.display_name).bind(&req.title)
    .bind(file_id).bind(REQ_SYSTEM).bind(&user.0.id)
    .execute(&s.db).await;

    detail_inner(&s, &user.0, req_id).await.map(Json)
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(rename = "box")]
    pub boxsel: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ListRow {
    id: Uuid,
    kind: String,
    title: String,
    amount: Option<f64>,
    created_by: Option<String>,
    requester_name: Option<String>,
    created_at: DateTime<Utc>,
    wf_state: Option<String>,
    next_reviewer: Option<String>,
}

#[derive(Serialize)]
pub struct RequestListItem {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub amount: Option<f64>,
    pub status: String,
    pub my_turn: bool,
    pub requester_id: Option<String>,
    pub requester_name: String,
    pub created_at: DateTime<Utc>,
}

fn status_of(wf_state: Option<&str>) -> String {
    match wf_state {
        Some("Approved") => "approved",
        Some("Rejected") => "rejected",
        Some("Review") => "in_review",
        _ => "submitted",
    }
    .to_string()
}

fn to_item(r: ListRow, me: &str) -> RequestListItem {
    let my_turn = r.next_reviewer.as_deref() == Some(me);
    RequestListItem {
        id: r.id,
        kind: r.kind,
        title: r.title,
        amount: r.amount,
        status: status_of(r.wf_state.as_deref()),
        my_turn,
        requester_id: r.created_by,
        requester_name: r.requester_name.unwrap_or_else(|| "Unknown".into()),
        created_at: r.created_at,
    }
}

pub async fn list(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<RequestListItem>>> {
    let me = user.0.id.clone();
    let is_admin = user.0.role == "admin";
    let boxsel = q.boxsel.as_deref().unwrap_or("mine");

    const BASE: &str = "SELECT r.id, r.kind, r.title, r.amount, r.created_by, \
        u.display_name AS requester_name, r.created_at, wf.state AS wf_state, \
        (SELECT s.reviewer_id FROM workflow_steps s \
           WHERE s.workflow_id = wf.id AND s.decision = 'pending' \
           ORDER BY s.sequence LIMIT 1) AS next_reviewer \
        FROM requests r \
        LEFT JOIN users u ON u.id = r.created_by \
        LEFT JOIN file_workflows wf ON wf.file_id = r.file_id";
    const INBOX: &str = "EXISTS (SELECT 1 FROM file_workflows wf2 \
        JOIN workflow_steps s2 ON s2.workflow_id = wf2.id \
        WHERE wf2.file_id = r.file_id AND s2.reviewer_id = $1 AND s2.decision = 'pending')";

    let rows: Vec<ListRow> = match boxsel {
        "inbox" => {
            sqlx::query_as(&format!("{BASE} WHERE {INBOX} ORDER BY r.created_at DESC"))
                .bind(&me).fetch_all(&s.db).await?
        }
        "all" if is_admin => {
            sqlx::query_as(&format!("{BASE} ORDER BY r.created_at DESC"))
                .fetch_all(&s.db).await?
        }
        "all" => {
            sqlx::query_as(&format!("{BASE} WHERE r.created_by = $1 OR {INBOX} ORDER BY r.created_at DESC"))
                .bind(&me).fetch_all(&s.db).await?
        }
        _ => {
            sqlx::query_as(&format!("{BASE} WHERE r.created_by = $1 ORDER BY r.created_at DESC"))
                .bind(&me).fetch_all(&s.db).await?
        }
    };

    Ok(Json(rows.into_iter().map(|r| to_item(r, &me)).collect()))
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------
#[derive(Serialize, sqlx::FromRow)]
pub struct FileLite {
    pub id: Uuid,
    pub name: String,
    pub file_type: String,
    pub size_bytes: i64,
}

#[derive(Serialize)]
pub struct RequestDetail {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub form_data: Value,
    pub amount: Option<f64>,
    pub ai_summary: Option<String>,
    pub status: String,
    pub order_mode: String,
    pub requester_id: Option<String>,
    pub requester_name: String,
    pub created_at: DateTime<Utc>,
    pub file: Option<FileLite>,
    pub steps: Vec<WorkflowStep>,
}

#[derive(sqlx::FromRow)]
struct RequestRow {
    id: Uuid,
    kind: String,
    title: String,
    form_data: Value,
    amount: Option<f64>,
    file_id: Option<Uuid>,
    ai_summary: Option<String>,
    created_by: Option<String>,
    created_at: DateTime<Utc>,
}

async fn detail_inner(s: &AppState, user: &crate::auth::User, req_id: Uuid) -> ApiResult<RequestDetail> {
    let r: RequestRow = sqlx::query_as(
        "SELECT id, kind, title, form_data, amount, file_id, ai_summary, created_by, created_at \
           FROM requests WHERE id = $1",
    )
    .bind(req_id).fetch_optional(&s.db).await?
    .ok_or(ApiError::NotFound)?;

    // Access: requester, an admin, or an assigned reviewer. Otherwise 404 (no
    // existence leak), matching the read semantics elsewhere.
    let is_requester = r.created_by.as_deref() == Some(user.id.as_str());
    let is_admin = user.role == "admin";
    let is_reviewer = if let Some(fid) = r.file_id {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM file_workflows wf \
             JOIN workflow_steps st ON st.workflow_id = wf.id \
             WHERE wf.file_id = $1 AND st.reviewer_id = $2)",
        )
        .bind(fid).bind(&user.id).fetch_one(&s.db).await.unwrap_or(false)
    } else {
        false
    };
    if !(is_requester || is_admin || is_reviewer) {
        return Err(ApiError::NotFound);
    }

    let requester_name = match &r.created_by {
        Some(uid) => sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
            .bind(uid).fetch_optional(&s.db).await?.unwrap_or_else(|| "Unknown".into()),
        None => "Unknown".into(),
    };

    let file = match r.file_id {
        Some(fid) => sqlx::query_as::<_, FileLite>(
            "SELECT id, name, file_type, size_bytes FROM files WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(fid).fetch_optional(&s.db).await?,
        None => None,
    };

    // Latest workflow on the anchor + its steps (with reviewer display names).
    let (wf, steps): (Option<Workflow>, Vec<WorkflowStep>) = match r.file_id {
        Some(fid) => {
            let wf: Option<Workflow> = sqlx::query_as(
                "SELECT * FROM file_workflows WHERE file_id = $1 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(fid).fetch_optional(&s.db).await?;
            let steps = match &wf {
                Some(w) => sqlx::query_as::<_, WorkflowStep>(
                    "SELECT s.*, COALESCE(u.display_name, 'Unknown reviewer') AS reviewer_name \
                       FROM workflow_steps s LEFT JOIN users u ON u.id = s.reviewer_id \
                      WHERE s.workflow_id = $1 ORDER BY s.sequence",
                )
                .bind(w.id).fetch_all(&s.db).await?,
                None => vec![],
            };
            (wf, steps)
        }
        None => (None, vec![]),
    };

    let status = status_of(wf.as_ref().map(|w| w.state.as_str()));
    let order_mode = wf.as_ref().map(|w| w.order_mode.clone()).unwrap_or_else(|| "sequential".into());

    Ok(RequestDetail {
        id: r.id,
        kind: r.kind,
        title: r.title,
        form_data: r.form_data,
        amount: r.amount,
        ai_summary: r.ai_summary,
        status,
        order_mode,
        requester_id: r.created_by,
        requester_name,
        created_at: r.created_at,
        file,
        steps,
    })
}

pub async fn detail(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RequestDetail>> {
    detail_inner(&s, &user.0, id).await.map(Json)
}

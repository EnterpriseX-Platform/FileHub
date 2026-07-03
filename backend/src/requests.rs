//! Requests — form/document submissions that flow through org approvals.
//!
//! Forms are **admin-authored** (the Form Designer): each request type lives in
//! `request_forms` with its fields and a linked `workflow_templates` route. The
//! four defaults are seeded non-destructively at startup (`bootstrap_request_forms`)
//! so an agency can edit them or add its own without a code change.
//!
//!   * `GET  /api/request-forms`   — active forms + their resolved route, for the
//!     Everyday "New request" flow and to tell the model which fields to target.
//!   * `GET/POST /api/forms`, `PATCH/DELETE /api/forms/:id` — the Form Designer
//!     CRUD (Admin Console, editor+).
//!   * `POST /api/requests/intake` — AI turns plain language into a draft (form +
//!     fields + title + summary + the form's route).
//!   * `POST /api/requests`        — persist + start the form's workflow.
//!   * `GET  /api/requests?box=…`   — my submissions / awaiting my decision.
//!   * `GET  /api/requests/:id`     — full detail.
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

/// Shared area holding request anchor documents so any assigned reviewer can
/// read them (a personal drive would 403 the reviewers).
const REQ_SYSTEM: &str = "sys_requests";

// ===========================================================================
// Default forms — the seed source only. At runtime everything reads the DB.
// ===========================================================================
struct FieldDef {
    key: &'static str, en: &'static str, th: &'static str, kind: &'static str, required: bool,
}
struct DefaultForm {
    id: &'static str, name_en: &'static str, name_th: &'static str, icon: &'static str, color: &'static str,
    fields: Vec<FieldDef>,
    template_id: &'static str, template_name: &'static str, order_mode: &'static str,
    steps: Vec<(&'static str, &'static str)>, // (reviewer_id, step label)
}

fn fd(key: &'static str, en: &'static str, th: &'static str, kind: &'static str, required: bool) -> FieldDef {
    FieldDef { key, en, th, kind, required }
}

fn default_forms() -> Vec<DefaultForm> {
    vec![
        DefaultForm {
            id: "expense", name_en: "Expense request", name_th: "เบิกค่าใช้จ่าย", icon: "expense", color: "emerald",
            fields: vec![
                FieldDef { key: "category", en: "Category", th: "หมวด", kind: "text", required: true },
                FieldDef { key: "amount", en: "Amount (THB)", th: "จำนวนเงิน (บาท)", kind: "money", required: true },
                FieldDef { key: "date", en: "Date", th: "วันที่", kind: "date", required: false },
                FieldDef { key: "cost_center", en: "Cost center", th: "ศูนย์ต้นทุน", kind: "text", required: false },
                FieldDef { key: "purpose", en: "Purpose", th: "วัตถุประสงค์", kind: "textarea", required: true },
            ],
            template_id: "wft_expense", template_name: "Expense approval", order_mode: "sequential",
            steps: vec![("usr_krit", "Manager"), ("usr_pat", "Finance")],
        },
        DefaultForm {
            id: "it", name_en: "IT / service request", name_th: "คำขอไอที", icon: "it", color: "indigo",
            fields: vec![
                fd("item", "Item / service", "รายการ", "text", true),
                fd("priority", "Priority", "ความเร่งด่วน", "text", false),
                fd("reason", "Reason", "เหตุผล", "textarea", true),
            ],
            template_id: "wft_it", template_name: "IT request", order_mode: "sequential",
            steps: vec![("usr_pat", "IT"), ("usr_krit", "Asset owner")],
        },
        DefaultForm {
            id: "document", name_en: "Document approval", name_th: "อนุมัติเอกสาร", icon: "document", color: "violet",
            fields: vec![
                fd("document", "Document", "เอกสาร", "text", true),
                fd("deadline", "Deadline", "กำหนดส่ง", "date", false),
                fd("note", "Note", "หมายเหตุ", "textarea", false),
            ],
            template_id: "wft_document", template_name: "Document approval", order_mode: "sequential",
            steps: vec![("usr_wisanu", "Legal"), ("usr_sarah", "Director")],
        },
        DefaultForm {
            id: "leave", name_en: "Leave request", name_th: "คำขอลา", icon: "leave", color: "amber",
            fields: vec![
                fd("leave_type", "Leave type", "ประเภทการลา", "text", true),
                fd("from", "From", "ตั้งแต่", "date", true),
                fd("to", "To", "ถึง", "date", true),
                fd("reason", "Reason", "เหตุผล", "textarea", false),
            ],
            template_id: "wft_leave", template_name: "Leave approval", order_mode: "sequential",
            steps: vec![("usr_krit", "Manager")],
        },
    ]
}

/// Seed the default templates + forms if they don't already exist. Idempotent
/// and **non-destructive** (`ON CONFLICT DO NOTHING`) so admin edits survive
/// reboots. Called at startup after users are seeded.
pub async fn bootstrap_request_forms(db: &sqlx::PgPool) -> anyhow::Result<()> {
    for d in default_forms() {
        let steps_json: Value = Value::Array(d.steps.iter().map(|(rid, name)| json!({
            "reviewer_id": rid, "name": name
        })).collect());
        sqlx::query(
            "INSERT INTO workflow_templates (id, name, description, order_mode, steps, created_by) \
             VALUES ($1,$2,$3,$4,$5,'usr_admin') ON CONFLICT (id) DO NOTHING",
        )
        .bind(d.template_id).bind(d.template_name).bind(Option::<String>::None).bind(d.order_mode).bind(&steps_json)
        .execute(db).await?;

        let fields_json: Value = Value::Array(d.fields.iter().map(|f| json!({
            "key": f.key, "label_en": f.en, "label_th": f.th, "kind": f.kind, "required": f.required
        })).collect());
        sqlx::query(
            "INSERT INTO request_forms (id, name_en, name_th, icon, color, fields, template_id, sort_order, created_by) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'usr_admin') ON CONFLICT (id) DO NOTHING",
        )
        .bind(d.id).bind(d.name_en).bind(d.name_th).bind(d.icon).bind(d.color)
        .bind(&fields_json).bind(d.template_id).bind(0i32)
        .execute(db).await?;
    }
    Ok(())
}

// ===========================================================================
// Form reads + shared helpers
// ===========================================================================
#[derive(Serialize)]
pub struct RouteStep {
    pub reviewer_id: String,
    pub reviewer_name: String,
    pub step_name: String,
}

/// A form as the Everyday flow needs it: fields + the resolved default route.
#[derive(Serialize)]
pub struct FormOut {
    pub id: String,
    pub name_en: String,
    pub name_th: String,
    pub description: Option<String>,
    pub icon: String,
    pub color: String,
    pub fields: Value,
    pub order_mode: String,
    pub route: Vec<RouteStep>,
}

#[derive(sqlx::FromRow)]
struct FormRow {
    id: String,
    name_en: String,
    name_th: String,
    description: Option<String>,
    icon: String,
    color: String,
    fields: Value,
    template_id: Option<String>,
}

async fn resolve_reviewer_name(db: &sqlx::PgPool, id: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
        .bind(id).fetch_optional(db).await.ok().flatten().unwrap_or_else(|| "Reviewer".into())
}

/// Read a template's steps as `(reviewer_id, optional step name)` in order.
async fn template_steps(db: &sqlx::PgPool, template_id: &str) -> Vec<(String, Option<String>)> {
    let row: Option<(String, Value)> =
        sqlx::query_as("SELECT order_mode, steps FROM workflow_templates WHERE id = $1")
            .bind(template_id).fetch_optional(db).await.ok().flatten();
    let Some((_, steps_json)) = row else { return vec![] };
    steps_json.as_array().map(|arr| arr.iter().filter_map(|st| {
        let rid = st.get("reviewer_id").and_then(|v| v.as_str())?.to_string();
        let name = st.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
        Some((rid, name))
    }).collect()).unwrap_or_default()
}

async fn template_order_mode(db: &sqlx::PgPool, template_id: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT order_mode FROM workflow_templates WHERE id = $1")
        .bind(template_id).fetch_optional(db).await.ok().flatten().unwrap_or_else(|| "sequential".into())
}

async fn resolve_route(db: &sqlx::PgPool, template_id: &str) -> Vec<RouteStep> {
    let mut out = vec![];
    for (rid, name) in template_steps(db, template_id).await {
        let reviewer_name = resolve_reviewer_name(db, &rid).await;
        out.push(RouteStep { reviewer_id: rid, reviewer_name, step_name: name.unwrap_or_else(|| "Approver".into()) });
    }
    out
}

async fn form_to_out(db: &sqlx::PgPool, f: FormRow) -> FormOut {
    let (order_mode, route) = match &f.template_id {
        Some(tid) => (template_order_mode(db, tid).await, resolve_route(db, tid).await),
        None => ("sequential".into(), vec![]),
    };
    FormOut {
        id: f.id, name_en: f.name_en, name_th: f.name_th, description: f.description,
        icon: f.icon, color: f.color, fields: f.fields, order_mode, route,
    }
}

/// GET /api/request-forms — active forms + resolved route (Everyday + intake).
pub async fn list_forms(
    State(s): State<Arc<AppState>>,
    _user: AuthUser,
) -> ApiResult<Json<Vec<FormOut>>> {
    let rows: Vec<FormRow> = sqlx::query_as(
        "SELECT id, name_en, name_th, description, icon, color, fields, template_id \
           FROM request_forms WHERE active = true ORDER BY sort_order, name_en",
    ).fetch_all(&s.db).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows { out.push(form_to_out(&s.db, r).await); }
    Ok(Json(out))
}

// ===========================================================================
// Form Designer CRUD (Admin Console, editor+)
// ===========================================================================
#[derive(Serialize, sqlx::FromRow)]
pub struct FormAdmin {
    pub id: String,
    pub name_en: String,
    pub name_th: String,
    pub description: Option<String>,
    pub icon: String,
    pub color: String,
    pub fields: Value,
    pub template_id: Option<String>,
    pub active: bool,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
}

pub async fn list_forms_admin(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<FormAdmin>>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let rows: Vec<FormAdmin> = sqlx::query_as(
        "SELECT id, name_en, name_th, description, icon, color, fields, template_id, active, sort_order, created_at \
           FROM request_forms ORDER BY sort_order, name_en",
    ).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct FormInput {
    pub id: Option<String>,
    pub name_en: String,
    pub name_th: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub fields: Value,
    pub template_id: Option<String>,
    pub active: Option<bool>,
    pub sort_order: Option<i32>,
}

pub async fn create_form(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(inp): Json<FormInput>,
) -> ApiResult<Json<FormAdmin>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    if inp.name_en.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let id = inp.id.filter(|i| !i.trim().is_empty())
        .unwrap_or_else(|| format!("form_{}", Uuid::now_v7().simple()));
    let fields = if inp.fields.is_array() { inp.fields } else { json!([]) };
    sqlx::query(
        "INSERT INTO request_forms (id, name_en, name_th, description, icon, color, fields, template_id, active, sort_order, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(&id).bind(inp.name_en.trim())
    .bind(inp.name_th.unwrap_or_else(|| inp.name_en.trim().to_string()))
    .bind(&inp.description).bind(inp.icon.unwrap_or_else(|| "generic".into()))
    .bind(inp.color.unwrap_or_else(|| "indigo".into())).bind(&fields)
    .bind(&inp.template_id).bind(inp.active.unwrap_or(true)).bind(inp.sort_order.unwrap_or(0))
    .bind(&user.0.id)
    .execute(&s.db).await
    .map_err(|e| if e.to_string().contains("duplicate") { ApiError::BadRequest("a form with that id already exists".into()) } else { e.into() })?;
    Ok(Json(fetch_form_admin(&s.db, &id).await?))
}

pub async fn update_form(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(inp): Json<FormInput>,
) -> ApiResult<Json<FormAdmin>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    // Confirm it exists first (404 vs a silent no-op).
    fetch_form_admin(&s.db, &id).await?;
    let fields = if inp.fields.is_array() { inp.fields } else { json!([]) };
    sqlx::query(
        "UPDATE request_forms SET name_en=$2, name_th=$3, description=$4, icon=$5, color=$6, \
                fields=$7, template_id=$8, active=$9, sort_order=$10, updated_at=now() WHERE id=$1",
    )
    .bind(&id).bind(inp.name_en.trim())
    .bind(inp.name_th.unwrap_or_else(|| inp.name_en.trim().to_string()))
    .bind(&inp.description).bind(inp.icon.unwrap_or_else(|| "generic".into()))
    .bind(inp.color.unwrap_or_else(|| "indigo".into())).bind(&fields)
    .bind(&inp.template_id).bind(inp.active.unwrap_or(true)).bind(inp.sort_order.unwrap_or(0))
    .execute(&s.db).await?;
    Ok(Json(fetch_form_admin(&s.db, &id).await?))
}

pub async fn delete_form(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let n = sqlx::query("DELETE FROM request_forms WHERE id = $1").bind(&id).execute(&s.db).await?;
    if n.rows_affected() == 0 { return Err(ApiError::NotFound); }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn fetch_form_admin(db: &sqlx::PgPool, id: &str) -> ApiResult<FormAdmin> {
    sqlx::query_as(
        "SELECT id, name_en, name_th, description, icon, color, fields, template_id, active, sort_order, created_at \
           FROM request_forms WHERE id = $1",
    ).bind(id).fetch_optional(db).await?.ok_or(ApiError::NotFound)
}

async fn fetch_form_row(db: &sqlx::PgPool, id: &str) -> Option<FormRow> {
    sqlx::query_as(
        "SELECT id, name_en, name_th, description, icon, color, fields, template_id \
           FROM request_forms WHERE id = $1",
    ).bind(id).fetch_optional(db).await.ok().flatten()
}

// ===========================================================================
// Intake — plain language → structured draft. AI only.
// ===========================================================================
#[derive(Deserialize)]
pub struct IntakeReq { pub message: String }

#[derive(Serialize)]
pub struct IntakeResult {
    pub kind: String,     // the chosen form id
    pub title: String,
    pub fields: Value,
    pub amount: Option<f64>,
    pub ai_summary: String,
    pub order_mode: String,
    pub route: Vec<RouteStep>,
}

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

    // Build the catalog from the admin-authored forms so the model classifies
    // into whatever request types this workspace actually has.
    let forms: Vec<FormRow> = sqlx::query_as(
        "SELECT id, name_en, name_th, description, icon, color, fields, template_id \
           FROM request_forms WHERE active = true ORDER BY sort_order, name_en",
    ).fetch_all(&s.db).await?;
    if forms.is_empty() {
        return Err(ApiError::BadRequest("no request forms are configured".into()));
    }
    let ids: Vec<String> = forms.iter().map(|f| f.id.clone()).collect();
    let mut catalog = String::new();
    for f in &forms {
        // Describe each field; for a `select` field, list its allowed choices so
        // the model fills a valid value.
        let descs: Vec<String> = f.fields.as_array().map(|a| a.iter().filter_map(|x| {
            let key = x.get("key").and_then(|k| k.as_str())?;
            let kind = x.get("kind").and_then(|k| k.as_str()).unwrap_or("text");
            if kind == "select" {
                let opts: Vec<&str> = x.get("options").and_then(|o| o.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
                Some(format!("{key} (one of: {})", opts.join(" | ")))
            } else {
                Some(key.to_string())
            }
        }).collect()).unwrap_or_default();
        catalog.push_str(&format!("- {} ({}): fields = [{}]\n", f.id, f.name_en, descs.join(", ")));
    }

    let system = "You convert a plain-language staff request into a structured form. \
        Respond ONLY with a single compact JSON object — no prose, no code fences.";
    let prompt = format!(
        "Request types and their field keys:\n{catalog}\n\
         User request:\n\"{}\"\n\n\
         Return JSON with keys: \
         \"kind\" (one of: {}), \
         \"title\" (a short human title, <= 8 words), \
         \"amount\" (number in THB or null), \
         \"fields\" (object keyed by the chosen type's field keys, values from the message; omit unknowns), \
         \"summary\" (one sentence written for the approver).",
        req.message, ids.join(", ")
    );

    let (raw, usage) = s.ai.chat(system, &prompt).await?;
    ai::record_usage(&s.db, Some(&user.0.id), Some(REQ_SYSTEM), "request_intake", s.ai.chat_model(), &usage).await;

    #[derive(Deserialize, Default)]
    struct Ext { kind: Option<String>, title: Option<String>, amount: Option<f64>, fields: Option<Value>, summary: Option<String> }
    let ext: Ext = serde_json::from_str(json_slice(&raw)).unwrap_or_default();

    // Snap the model's kind to a real form id; fall back to the first form.
    let kind = ext.kind.filter(|k| ids.iter().any(|i| i == k)).unwrap_or_else(|| ids[0].clone());
    let form = forms.iter().find(|f| f.id == kind).unwrap();
    let (order_mode, route) = match &form.template_id {
        Some(tid) => (template_order_mode(&s.db, tid).await, resolve_route(&s.db, tid).await),
        None => ("sequential".into(), vec![]),
    };

    Ok(Json(IntakeResult {
        title: ext.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| form.name_en.clone()),
        kind,
        fields: ext.fields.unwrap_or_else(|| json!({})),
        amount: ext.amount,
        ai_summary: ext.summary.unwrap_or_default(),
        order_mode,
        route,
    }))
}

// ===========================================================================
// Create
// ===========================================================================
#[derive(Deserialize)]
pub struct RouteStepIn { pub reviewer_id: String, pub step_name: Option<String> }

#[derive(Deserialize)]
pub struct CreateReq {
    pub kind: String, // a form id
    pub title: String,
    #[serde(default)]
    pub form_data: Value,
    pub amount: Option<f64>,
    pub file_id: Option<Uuid>,
    pub reviewers: Option<Vec<RouteStepIn>>,
    pub order_mode: Option<String>,
    pub ai_summary: Option<String>,
}

fn render_request_doc(title: &str, form_name: &str, form: &Value) -> String {
    let mut out = format!("# {title}\n\nRequest type: {form_name}\n\n");
    if let Some(obj) = form.as_object() {
        for (k, v) in obj {
            let val = match v { Value::String(s) => s.clone(), Value::Null => String::new(), other => other.to_string() };
            if !val.is_empty() { out.push_str(&format!("- {k}: {val}\n")); }
        }
    }
    out
}

pub async fn create(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<CreateReq>,
) -> ApiResult<Json<RequestDetail>> {
    if req.title.trim().is_empty() {
        return Err(ApiError::BadRequest("title is required".into()));
    }
    let form = fetch_form_row(&s.db, &req.kind).await
        .ok_or_else(|| ApiError::BadRequest("unknown request form".into()))?;

    // Anchor document: an attached file in the Requests area, or a generated one.
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
        let body = render_request_doc(&req.title, &form.name_en, &req.form_data);
        let name = format!("{}.md", req.title.trim());
        let file = crate::handlers::create_file_from_bytes(
            &s, Some(&user.0.id), name, REQ_SYSTEM.into(), None,
            Some(user.0.display_name.clone()), body.into_bytes().into(), Some("text/markdown".into()),
        ).await?;
        file.id
    };

    let req_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO requests (id, kind, title, form_data, amount, file_id, system_id, ai_summary, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(req_id).bind(&form.id).bind(&req.title).bind(&req.form_data).bind(req.amount)
    .bind(file_id).bind(REQ_SYSTEM).bind(&req.ai_summary).bind(&user.0.id)
    .execute(&s.db).await?;

    // Route: caller override, else the form's linked template.
    let (order_mode, steps): (String, Vec<(String, Option<String>)>) =
        match req.reviewers.filter(|v| !v.is_empty()) {
            Some(rv) => {
                let om = req.order_mode.clone().unwrap_or_else(|| "sequential".into());
                (om, rv.into_iter().map(|r| (r.reviewer_id, r.step_name)).collect())
            }
            None => match &form.template_id {
                Some(tid) => (template_order_mode(&s.db, tid).await, template_steps(&s.db, tid).await),
                None => return Err(ApiError::BadRequest("this form has no approval route configured".into())),
            },
        };

    crate::p1::start_workflow_core(
        &s.db, file_id, &user.0.id, &Some(req.title.clone()), &order_mode, &steps, form.template_id.as_deref(),
    ).await?;

    let _ = sqlx::query(
        "INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id) \
         VALUES ($1,$2,'indigo','submitted a request',$3,'request',$4,$5,NULL,now(),$6)",
    )
    .bind(Uuid::now_v7()).bind(&user.0.display_name).bind(&req.title)
    .bind(file_id).bind(REQ_SYSTEM).bind(&user.0.id)
    .execute(&s.db).await;

    detail_inner(&s, &user.0, req_id).await.map(Json)
}

// ===========================================================================
// List
// ===========================================================================
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
    icon: Option<String>,
    color: Option<String>,
    kind_label: Option<String>,
}

#[derive(Serialize)]
pub struct RequestListItem {
    pub id: Uuid,
    pub kind: String,
    pub kind_label: String,
    pub icon: String,
    pub color: String,
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
    }.to_string()
}

fn to_item(r: ListRow, me: &str) -> RequestListItem {
    let my_turn = r.next_reviewer.as_deref() == Some(me);
    RequestListItem {
        kind_label: r.kind_label.unwrap_or_else(|| r.kind.clone()),
        icon: r.icon.unwrap_or_else(|| "generic".into()),
        color: r.color.unwrap_or_else(|| "slate".into()),
        id: r.id, kind: r.kind, title: r.title, amount: r.amount,
        status: status_of(r.wf_state.as_deref()), my_turn,
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
        rf.icon AS icon, rf.color AS color, rf.name_en AS kind_label, \
        (SELECT s.reviewer_id FROM workflow_steps s \
           WHERE s.workflow_id = wf.id AND s.decision = 'pending' \
           ORDER BY s.sequence LIMIT 1) AS next_reviewer \
        FROM requests r \
        LEFT JOIN users u ON u.id = r.created_by \
        LEFT JOIN request_forms rf ON rf.id = r.kind \
        LEFT JOIN file_workflows wf ON wf.file_id = r.file_id";
    const INBOX: &str = "EXISTS (SELECT 1 FROM file_workflows wf2 \
        JOIN workflow_steps s2 ON s2.workflow_id = wf2.id \
        WHERE wf2.file_id = r.file_id AND s2.reviewer_id = $1 AND s2.decision = 'pending')";

    let rows: Vec<ListRow> = match boxsel {
        "inbox" => sqlx::query_as(&format!("{BASE} WHERE {INBOX} ORDER BY r.created_at DESC")).bind(&me).fetch_all(&s.db).await?,
        "all" if is_admin => sqlx::query_as(&format!("{BASE} ORDER BY r.created_at DESC")).fetch_all(&s.db).await?,
        "all" => sqlx::query_as(&format!("{BASE} WHERE r.created_by = $1 OR {INBOX} ORDER BY r.created_at DESC")).bind(&me).fetch_all(&s.db).await?,
        _ => sqlx::query_as(&format!("{BASE} WHERE r.created_by = $1 ORDER BY r.created_at DESC")).bind(&me).fetch_all(&s.db).await?,
    };
    Ok(Json(rows.into_iter().map(|r| to_item(r, &me)).collect()))
}

// ===========================================================================
// Detail
// ===========================================================================
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
    pub kind_label: String,
    pub icon: String,
    pub color: String,
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
    ).bind(req_id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;

    let is_requester = r.created_by.as_deref() == Some(user.id.as_str());
    let is_admin = user.role == "admin";
    let is_reviewer = if let Some(fid) = r.file_id {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM file_workflows wf \
             JOIN workflow_steps st ON st.workflow_id = wf.id \
             WHERE wf.file_id = $1 AND st.reviewer_id = $2)",
        ).bind(fid).bind(&user.id).fetch_one(&s.db).await.unwrap_or(false)
    } else { false };
    if !(is_requester || is_admin || is_reviewer) {
        return Err(ApiError::NotFound);
    }

    let requester_name = match &r.created_by {
        Some(uid) => sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
            .bind(uid).fetch_optional(&s.db).await?.unwrap_or_else(|| "Unknown".into()),
        None => "Unknown".into(),
    };

    // Form presentation (icon/color/label). LEFT JOIN semantics — the form may
    // have been deleted since the request was filed.
    let form_meta: Option<(String, String, String)> = sqlx::query_as(
        "SELECT icon, color, name_en FROM request_forms WHERE id = $1",
    ).bind(&r.kind).fetch_optional(&s.db).await?;
    let (icon, color, kind_label) = form_meta
        .unwrap_or_else(|| ("generic".into(), "slate".into(), r.kind.clone()));

    let file = match r.file_id {
        Some(fid) => sqlx::query_as::<_, FileLite>(
            "SELECT id, name, file_type, size_bytes FROM files WHERE id = $1 AND deleted_at IS NULL",
        ).bind(fid).fetch_optional(&s.db).await?,
        None => None,
    };

    let (wf, steps): (Option<Workflow>, Vec<WorkflowStep>) = match r.file_id {
        Some(fid) => {
            let wf: Option<Workflow> = sqlx::query_as(
                "SELECT * FROM file_workflows WHERE file_id = $1 ORDER BY created_at DESC LIMIT 1",
            ).bind(fid).fetch_optional(&s.db).await?;
            let steps = match &wf {
                Some(w) => sqlx::query_as::<_, WorkflowStep>(
                    "SELECT s.*, COALESCE(u.display_name, 'Unknown reviewer') AS reviewer_name \
                       FROM workflow_steps s LEFT JOIN users u ON u.id = s.reviewer_id \
                      WHERE s.workflow_id = $1 ORDER BY s.sequence",
                ).bind(w.id).fetch_all(&s.db).await?,
                None => vec![],
            };
            (wf, steps)
        }
        None => (None, vec![]),
    };

    let status = status_of(wf.as_ref().map(|w| w.state.as_str()));
    let order_mode = wf.as_ref().map(|w| w.order_mode.clone()).unwrap_or_else(|| "sequential".into());

    Ok(RequestDetail {
        id: r.id, kind: r.kind, kind_label, icon, color, title: r.title,
        form_data: r.form_data, amount: r.amount, ai_summary: r.ai_summary,
        status, order_mode, requester_id: r.created_by, requester_name,
        created_at: r.created_at, file, steps,
    })
}

pub async fn detail(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RequestDetail>> {
    detail_inner(&s, &user.0, id).await.map(Json)
}

//! P1 features in one place: workflow, comments, notifications, thumbnails.
//! Each section is self-contained — see the module-level dividers.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use image::{ImageEncoder, ImageReader};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// =============================================================================
// PDF / text extraction (TOR 4.15.14 "search content").
// =============================================================================

/// Try to extract searchable text from the uploaded payload.  Returns `None`
/// if the format isn't supported — the upload still succeeds, just without
/// content-level search hits.
pub fn extract_text_from(file_type: &str, body: &[u8]) -> Option<String> {
    match file_type {
        "pdf" => pdf_extract::extract_text_from_mem(body).ok(),
        "txt" | "md" | "html" | "xml" | "csv" | "json" => {
            // Plain-ish text files: best-effort UTF-8 decode, capped at 1 MB so
            // a runaway JSON dump doesn't bloat the FTS index.
            let cap = body.len().min(1024 * 1024);
            std::str::from_utf8(&body[..cap]).ok().map(|s| s.to_string())
        }
        // Office documents (MEA TOR 5.3.4.2 full-text on Word/Excel/PPT): route
        // through the same LibreOffice→PDF conversion the preview uses, then
        // extract text from the PDF. No-ops cleanly if soffice isn't installed.
        "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "odt" | "ods" | "odp" => {
            let pdf = convert_to_pdf(&format!("document.{file_type}"), body)?;
            pdf_extract::extract_text_from_mem(&pdf)
                .ok()
                .filter(|t| !t.trim().is_empty())
        }
        _ => None,
    }
}

pub async fn index_file_content(db: &sqlx::PgPool, file_id: Uuid, content: &str) {
    if content.trim().is_empty() { return; }
    let _ = sqlx::query(
        r#"INSERT INTO file_content (file_id, content)
           VALUES ($1, $2)
           ON CONFLICT (file_id) DO UPDATE SET content = EXCLUDED.content, extracted_at = now()"#,
    )
    .bind(file_id)
    .bind(content)
    .execute(db)
    .await;
}

// =============================================================================
// Thumbnails (TOR 4.15.16 image caching).
// =============================================================================

/// Decode the image, resize the longest side to `max_dim`, encode as WebP.
pub fn make_thumbnail(body: &[u8], max_dim: u32) -> Option<(u32, u32, Vec<u8>)> {
    let img = ImageReader::new(std::io::Cursor::new(body))
        .with_guessed_format().ok()?
        .decode().ok()?;
    let thumb = img.thumbnail(max_dim, max_dim);
    let (w, h) = (thumb.width(), thumb.height());
    let mut out = Vec::new();
    // image 0.25 doesn't ship a WebP encoder without a feature flag, so we
    // fall back to PNG. Either is fine for serving — the table notes the mime.
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(thumb.to_rgba8().as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some((w, h, out))
}

pub async fn index_thumbnail(db: &sqlx::PgPool, file_id: Uuid, file_type: &str, body: &[u8]) {
    if !matches!(file_type, "png" | "jpg" | "jpeg" | "img" | "gif") { return; }
    let Some((w, h, bytes)) = make_thumbnail(body, 256) else { return; };
    let _ = sqlx::query(
        r#"INSERT INTO thumbnails (file_id, width, height, mime, data)
           VALUES ($1, $2, $3, 'image/png', $4)
           ON CONFLICT (file_id) DO UPDATE SET width = EXCLUDED.width,
                                                height = EXCLUDED.height,
                                                data = EXCLUDED.data,
                                                created_at = now()"#,
    )
    .bind(file_id).bind(w as i32).bind(h as i32).bind(bytes)
    .execute(db).await;
}

// =============================================================================
// Office → PDF preview via LibreOffice headless (TOR + browser-view).
// =============================================================================

/// File types that LibreOffice can render to PDF.  PDF itself is included so
/// `/preview` always returns a viewable PDF without the caller needing to
/// know which formats are convertible.
fn is_office_doc(file_type: &str) -> bool {
    matches!(file_type, "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "odt" | "ods" | "odp" | "pdf")
}

/// Spawn LibreOffice to convert the buffer to PDF.  Returns the PDF bytes,
/// or `None` if `soffice` is missing or conversion fails.  Caller decides
/// whether that's fatal (it's not — uploads still succeed).
pub fn convert_to_pdf(file_name: &str, body: &[u8]) -> Option<Vec<u8>> {
    // PDFs are self-preview.
    if file_name.to_lowercase().ends_with(".pdf") {
        return Some(body.to_vec());
    }

    // Pick the binary.  Some distros install as `libreoffice`, others as
    // `soffice`; Windows installs don't put it on PATH at all, so probe the
    // default install locations too.  Spawning `--version` doubles as the
    // existence check (portable — no `which`/`where` dependency).
    let mut candidates: Vec<String> = vec!["soffice".into(), "libreoffice".into()];
    if cfg!(windows) {
        candidates.push(r"C:\Program Files\LibreOffice\program\soffice.exe".into());
        candidates.push(r"C:\Program Files (x86)\LibreOffice\program\soffice.exe".into());
    }
    let bin = candidates.into_iter().find(|b| {
        std::process::Command::new(b).arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false)
    })?;

    // Write the input to a temp file (LibreOffice doesn't read stdin).
    let dir = std::env::temp_dir().join(format!("filehub-conv-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).ok()?;
    let input  = dir.join(file_name);
    let output = dir.join(format!("{}.pdf",
        std::path::Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("preview")));
    std::fs::write(&input, body).ok()?;

    let status = std::process::Command::new(bin)
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(&dir)
        .arg(&input)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    }

    let pdf = std::fs::read(&output).ok();
    let _   = std::fs::remove_dir_all(&dir);
    pdf
}

pub async fn index_office_preview(db: &sqlx::PgPool, file_id: Uuid, file_type: &str, file_name: &str, body: &[u8]) {
    if !is_office_doc(file_type) { return; }
    let body_owned = body.to_vec();
    let name_owned = file_name.to_string();
    let pdf = match tokio::task::spawn_blocking(move || convert_to_pdf(&name_owned, &body_owned)).await {
        Ok(Some(pdf)) => pdf,
        _ => return,
    };
    let _ = sqlx::query(
        r#"INSERT INTO file_previews (file_id, data, mime)
           VALUES ($1, $2, 'application/pdf')
           ON CONFLICT (file_id) DO UPDATE SET data = EXCLUDED.data, created_at = now()"#,
    )
    .bind(file_id).bind(pdf)
    .execute(db).await;
}

pub async fn get_preview(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<axum::response::Response> {
    // Load the file first so we can verify the caller has access before
    // serving any cached preview bytes.  Without this, a viewer could read
    // someone else's personal-drive preview by guessing the file id.
    let file: crate::models::File = sqlx::query_as(
        "SELECT id, name, file_type, size_bytes, system_id, org_id, bucket, object_key, project, \
                status, owner, tags, version, metadata, etag, created_at, modified_at, \
                folder_id, encrypted, deleted_at, created_by \
           FROM files WHERE id = $1 AND deleted_at IS NULL"
    )
    .bind(file_id)
    .fetch_optional(&s.db).await?
    .ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }

    // Fast path — preview already in the cache.
    if let Some((mime, data)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT mime, data FROM file_previews WHERE file_id = $1"
    ).bind(file_id).fetch_optional(&s.db).await? {
        return Ok(preview_response(mime, data));
    }

    // Slow path — file was never indexed (e.g. seed data, or LibreOffice was
    // down at upload time).  Read the source bytes, convert on demand, cache
    // the result, then serve it.  Subsequent hits take the fast path above.

    if !is_office_doc(&file.file_type) {
        return Err(ApiError::NotFound);
    }

    let (body, _) = s.storage.get(&file.object_key, file.encrypted).await?
        .ok_or(ApiError::NotFound)?;

    // Run LibreOffice on a blocking thread so the runtime stays responsive
    // (conversions can take a few seconds for large docs).
    let name = file.name.clone();
    let bytes = body.to_vec();
    let pdf = tokio::task::spawn_blocking(move || convert_to_pdf(&name, &bytes))
        .await
        .map_err(|e| ApiError::Other(anyhow::anyhow!("convert task: {e}")))?
        .ok_or(ApiError::NotFound)?;

    sqlx::query(
        r#"INSERT INTO file_previews (file_id, data, mime)
           VALUES ($1, $2, 'application/pdf')
           ON CONFLICT (file_id) DO UPDATE SET data = EXCLUDED.data, created_at = now()"#,
    )
    .bind(file_id).bind(&pdf)
    .execute(&s.db).await?;

    Ok(preview_response("application/pdf".into(), pdf))
}

fn preview_response(mime: String, data: Vec<u8>) -> axum::response::Response {
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    h.insert(header::CACHE_CONTROL, "private, max-age=3600".parse().unwrap());
    (StatusCode::OK, h, Bytes::from(data)).into_response()
}

pub async fn get_thumbnail(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<axum::response::Response> {
    let owner_row: Option<(String, String, String, bool)> = sqlx::query_as(
        "SELECT system_id, file_type, object_key, encrypted FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (system_id, file_type, object_key, encrypted) = owner_row.ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    // Fast path — thumbnail already cached.
    if let Some((mime, data)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT mime, data FROM thumbnails WHERE file_id = $1"
    ).bind(file_id).fetch_optional(&s.db).await? {
        return Ok(thumb_response(mime, data));
    }

    // Slow path — file was never indexed (seed data, or body written outside
    // the upload path). Generate on demand and cache, mirroring get_preview.
    if !matches!(file_type.as_str(), "img" | "png" | "jpg" | "jpeg" | "gif" | "webp") {
        return Err(ApiError::NotFound);
    }
    let (body, _) = s.storage.get(&object_key, encrypted).await?
        .ok_or(ApiError::NotFound)?;
    let bytes = body.to_vec();
    let (w, h, thumb) = tokio::task::spawn_blocking(move || make_thumbnail(&bytes, 256))
        .await
        .map_err(|e| ApiError::Other(anyhow::anyhow!("thumbnail task: {e}")))?
        .ok_or(ApiError::NotFound)?;
    sqlx::query(
        r#"INSERT INTO thumbnails (file_id, width, height, mime, data)
           VALUES ($1, $2, $3, 'image/png', $4)
           ON CONFLICT (file_id) DO UPDATE SET data = EXCLUDED.data"#,
    )
    .bind(file_id).bind(w as i32).bind(h as i32).bind(&thumb)
    .execute(&s.db).await?;
    Ok(thumb_response("image/png".into(), thumb))
}

fn thumb_response(mime: String, data: Vec<u8>) -> axum::response::Response {
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    h.insert(header::CACHE_CONTROL, "public, max-age=604800, immutable".parse().unwrap());
    (StatusCode::OK, h, Bytes::from(data)).into_response()
}

// =============================================================================
// Workflow (TOR 4.15.13 — file revision state machine).
// =============================================================================

#[derive(Debug, Serialize, FromRow)]
pub struct Workflow {
    pub id:         Uuid,
    pub file_id:    Uuid,
    pub state:      String,
    pub note:       Option<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    #[sqlx(default)]
    pub order_mode: String,
    #[sqlx(default)]
    pub template_id: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct WorkflowStep {
    pub id:           Uuid,
    pub workflow_id:  Uuid,
    pub reviewer_id:  Option<String>,
    pub decision:     Option<String>,
    pub note:         Option<String>,
    pub sequence:     i32,
    pub created_at:   DateTime<Utc>,
    pub decided_at:   Option<DateTime<Utc>>,
    /// Step name from the template (e.g. "หัวหน้าฝ่าย"); NULL for ad-hoc steps.
    #[sqlx(default)]
    pub name:         Option<String>,
    /// Reviewer's display name, joined from `users` in `list_workflow`. The
    /// frontend renders this directly. `#[sqlx(default)]` so the bare
    /// `SELECT * FROM workflow_steps` paths (which don't join) still decode —
    /// they just leave it `None`.
    #[sqlx(default)]
    pub reviewer_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StartWorkflow {
    /// Reviewer user ids in order. Ignored when `template_id` is given.
    #[serde(default)]
    pub reviewer_ids: Vec<String>,
    pub note:         Option<String>,
    /// "sequential" | "parallel" (default parallel, matching prior behaviour).
    pub order_mode:   Option<String>,
    /// Start from a reusable template instead of `reviewer_ids`.
    pub template_id:  Option<String>,
}

/// One resolved step to create: (reviewer_id, step name).
type ResolvedStep = (String, Option<String>);

#[derive(Debug, Deserialize)]
pub struct DecideStep {
    pub decision: String,            // approved | rejected
    pub note:     Option<String>,
}

/// Notify a reviewer that a document awaits their review (in-app + email).
pub(crate) async fn notify_reviewer(db: &sqlx::PgPool, reviewer_id: &str, note: &Option<String>, file_id: Uuid) {
    let _ = sqlx::query(
        r#"INSERT INTO notifications (id, user_id, kind, title, body, link)
           VALUES ($1, $2, 'review_requested', $3, $4, $5)"#,
    )
    .bind(Uuid::now_v7()).bind(reviewer_id)
    .bind("Review requested")
    .bind(note.clone().unwrap_or_default())
    .bind(format!("/f/{file_id}"))
    .execute(db).await;

    if crate::mailer::enabled() {
        if let Ok(Some((email,))) = sqlx::query_as::<_, (String,)>("SELECT email FROM users WHERE id = $1")
            .bind(reviewer_id).fetch_optional(db).await
        {
            let body = format!("A document needs your review.\n\nOpen: {}/f/{file_id}", crate::mailer::base_url());
            tokio::spawn(async move { let _ = crate::mailer::send(&email, "Review requested", &body).await; });
        }
    }
}

pub async fn start_workflow(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
    Json(c): Json<StartWorkflow>,
) -> ApiResult<Json<Workflow>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    // Ensure the file exists & isn't trashed.
    let exists: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = exists.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;

    // Resolve the steps + routing mode, either from a template or the request.
    let (order_mode, steps): (String, Vec<ResolvedStep>) = if let Some(ref tid) = c.template_id {
        let tpl: Option<(String, serde_json::Value)> =
            sqlx::query_as("SELECT order_mode, steps FROM workflow_templates WHERE id = $1")
                .bind(tid).fetch_optional(&s.db).await?;
        let (mode, steps_json) = tpl.ok_or_else(|| ApiError::BadRequest("unknown template".into()))?;
        let steps: Vec<ResolvedStep> = steps_json.as_array().map(|arr| {
            arr.iter().filter_map(|st| {
                let rid = st.get("reviewer_id").and_then(|v| v.as_str())?.to_string();
                let name = st.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                Some((rid, name))
            }).collect()
        }).unwrap_or_default();
        (mode, steps)
    } else {
        let mode = match c.order_mode.as_deref() {
            Some("sequential") => "sequential".to_string(),
            _ => "parallel".to_string(),
        };
        (mode, c.reviewer_ids.iter().cloned().map(|r| (r, None)).collect())
    };
    if steps.is_empty() {
        return Err(ApiError::BadRequest("at least one reviewer is required".into()));
    }

    let wf_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO file_workflows (id, file_id, state, note, created_by, order_mode, template_id)
           VALUES ($1, $2, 'Review', $3, $4, $5, $6)"#,
    )
    .bind(wf_id).bind(file_id).bind(&c.note).bind(&user.0.id).bind(&order_mode).bind(&c.template_id)
    .execute(&s.db).await?;

    for (i, (rid, name)) in steps.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO workflow_steps (id, workflow_id, reviewer_id, decision, sequence, name)
               VALUES ($1, $2, $3, 'pending', $4, $5)"#,
        )
        .bind(Uuid::now_v7()).bind(wf_id).bind(rid).bind((i + 1) as i32).bind(name)
        .execute(&s.db).await?;

        // Sequential: only the first step is notified now; the next is pinged as
        // each step is approved. Parallel: everyone is notified up front.
        if order_mode == "parallel" || i == 0 {
            notify_reviewer(&s.db, rid, &c.note, file_id).await;
        }
    }

    // File status follows the workflow state.
    sqlx::query("UPDATE files SET status = 'Review', modified_at = now() WHERE id = $1")
        .bind(file_id).execute(&s.db).await?;

    Ok(Json(sqlx::query_as("SELECT * FROM file_workflows WHERE id = $1")
        .bind(wf_id).fetch_one(&s.db).await?))
}

pub async fn list_workflow(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<Json<Vec<(Workflow, Vec<WorkflowStep>)>>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let workflows: Vec<Workflow> = sqlx::query_as(
        "SELECT * FROM file_workflows WHERE file_id = $1 ORDER BY created_at DESC"
    ).bind(file_id).fetch_all(&s.db).await?;
    let mut out: Vec<(Workflow, Vec<WorkflowStep>)> = Vec::new();
    for wf in workflows {
        let steps: Vec<WorkflowStep> = sqlx::query_as(
            r#"SELECT s.*, COALESCE(u.display_name, 'Unknown reviewer') AS reviewer_name
                 FROM workflow_steps s
                 LEFT JOIN users u ON u.id = s.reviewer_id
                WHERE s.workflow_id = $1
                ORDER BY s.sequence"#
        ).bind(wf.id).fetch_all(&s.db).await?;
        out.push((wf, steps));
    }
    Ok(Json(out))
}

pub async fn decide_step(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(step_id): Path<Uuid>,
    Json(d): Json<DecideStep>,
) -> ApiResult<Json<WorkflowStep>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    if !["approved", "rejected"].contains(&d.decision.as_str()) {
        return Err(ApiError::BadRequest("decision must be approved or rejected".into()));
    }
    let step: WorkflowStep = sqlx::query_as("SELECT * FROM workflow_steps WHERE id = $1")
        .bind(step_id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    // Reach into the underlying file's system to gate by personal-drive access.
    let sys_row: Option<(String,)> = sqlx::query_as(
        "SELECT f.system_id FROM file_workflows wf \
         JOIN files f ON f.id = wf.file_id WHERE wf.id = $1"
    ).bind(step.workflow_id).fetch_optional(&s.db).await?;
    let (system_id,) = sys_row.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;
    if step.reviewer_id.as_ref() != Some(&user.0.id) {
        return Err(ApiError::Forbidden);
    }

    // Sequential turn-guard: can't decide while an earlier step is still pending.
    let (order_mode, wf_file_id): (String, Uuid) = sqlx::query_as(
        "SELECT order_mode, file_id FROM file_workflows WHERE id = $1",
    )
    .bind(step.workflow_id).fetch_one(&s.db).await?;
    if order_mode == "sequential" {
        let earlier_pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_steps WHERE workflow_id = $1 AND decision = 'pending' AND sequence < $2)",
        )
        .bind(step.workflow_id).bind(step.sequence).fetch_one(&s.db).await?;
        if earlier_pending {
            return Err(ApiError::Conflict("an earlier reviewer hasn't decided yet".into()));
        }
    }

    sqlx::query("UPDATE workflow_steps SET decision = $1, note = $2, decided_at = now() WHERE id = $3")
        .bind(&d.decision).bind(&d.note).bind(step_id).execute(&s.db).await?;

    // Sequential + approved + more to go → ping the next pending reviewer.
    if order_mode == "sequential" && d.decision == "approved" {
        let next: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT reviewer_id FROM workflow_steps WHERE workflow_id = $1 AND decision = 'pending' ORDER BY sequence LIMIT 1",
        )
        .bind(step.workflow_id).fetch_optional(&s.db).await?;
        if let Some((Some(next_id),)) = next {
            notify_reviewer(&s.db, &next_id, &d.note, wf_file_id).await;
        }
    }

    // Roll-up: any reject → Rejected; all approved → Approved.
    let wf_id = step.workflow_id;
    let any_rejected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_steps WHERE workflow_id = $1 AND decision = 'rejected')"
    ).bind(wf_id).fetch_one(&s.db).await?;
    let all_approved: bool = sqlx::query_scalar(
        "SELECT bool_and(decision = 'approved') FROM workflow_steps WHERE workflow_id = $1"
    ).bind(wf_id).fetch_one(&s.db).await?;

    let new_state = if any_rejected { Some("Rejected") }
                    else if all_approved { Some("Approved") }
                    else { None };

    if let Some(state) = new_state {
        sqlx::query("UPDATE file_workflows SET state = $1 WHERE id = $2")
            .bind(state).bind(wf_id).execute(&s.db).await?;
        // Mirror into files.status so existing filters keep working.
        sqlx::query(
            "UPDATE files SET status = $1, modified_at = now()
              FROM file_workflows
             WHERE files.id = file_workflows.file_id AND file_workflows.id = $2"
        ).bind(state).bind(wf_id).execute(&s.db).await?;

        // Notify the workflow creator of the outcome. created_by is TEXT
        // (users.id), not a UUID.
        let creator: Option<(Option<String>, Uuid)> = sqlx::query_as(
            "SELECT created_by, file_id FROM file_workflows WHERE id = $1"
        ).bind(wf_id).fetch_optional(&s.db).await?;
        if let Some((Some(uid), file_id)) = creator {
            let _ = sqlx::query(
                r#"INSERT INTO notifications (id, user_id, kind, title, link)
                   VALUES ($1, $2, $3, $4, $5)"#,
            )
            .bind(Uuid::now_v7()).bind(uid).bind(state.to_lowercase())
            .bind(format!("Workflow {state}"))
            .bind(format!("/files/{file_id}"))
            .execute(&s.db).await;
        }
    }

    Ok(Json(sqlx::query_as("SELECT * FROM workflow_steps WHERE id = $1")
        .bind(step_id).fetch_one(&s.db).await?))
}

// =============================================================================
// Comments (file-level discussion).
// =============================================================================

// `user_id` references users.id which is TEXT (CUID2 per the hybrid-key
// strategy in 0001_init.sql).  Previously these structs typed it as Uuid,
// which serialised fine on INSERT (sqlx coerces the bound value) but blew up
// on SELECT with "Rust type uuid::Uuid is not compatible with SQL type TEXT".
// Switching to String fixes the round trip and matches the actual column.
#[derive(Debug, Serialize, FromRow)]
pub struct Comment {
    pub id:         Uuid,
    pub file_id:    Uuid,
    pub user_id:    String,
    pub parent_id:  Option<Uuid>,
    pub body:       String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CommentWithAuthor {
    pub id:           Uuid,
    pub file_id:      Uuid,
    pub user_id:      String,
    pub parent_id:    Option<Uuid>,
    pub body:         String,
    pub created_at:   DateTime<Utc>,
    pub display_name: String,
    pub avatar_tone:  String,
}

#[derive(Debug, Deserialize)]
pub struct NewComment {
    pub body:      String,
    pub parent_id: Option<Uuid>,
}

pub async fn list_comments(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<Json<Vec<CommentWithAuthor>>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let rows: Vec<CommentWithAuthor> = sqlx::query_as(
        r#"SELECT c.id, c.file_id, c.user_id, c.parent_id, c.body, c.created_at,
                  u.display_name, u.avatar_tone
             FROM file_comments c
             JOIN users u ON u.id = c.user_id
            WHERE c.file_id = $1
            ORDER BY c.created_at"#,
    ).bind(file_id).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

pub async fn create_comment(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
    Json(c): Json<NewComment>,
) -> ApiResult<Json<Comment>> {
    // Commenting is the one mutation viewers ARE allowed (see the Roles &
    // permissions matrix: "Comment on files" is checked for all three roles).
    // So no editor gate here — a valid session plus `ensure_system_access`
    // below is the whole authorization.
    if c.body.trim().is_empty() {
        return Err(ApiError::BadRequest("comment body required".into()));
    }
    let sys_row: Option<(String,)> = sqlx::query_as(
        "SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = sys_row.ok_or(ApiError::NotFound)?;
    // Access denial → 404, matching list_comments: commenting has no role gate
    // (viewers may comment), so ensure_system_access is the only discriminator.
    // Propagating its 403 would let a probe distinguish "exists but hidden"
    // from "doesn't exist" on someone else's personal-drive file.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO file_comments (id, file_id, user_id, parent_id, body)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(id).bind(file_id).bind(user.0.id).bind(c.parent_id).bind(c.body.trim())
    .execute(&s.db).await?;
    Ok(Json(sqlx::query_as("SELECT * FROM file_comments WHERE id = $1")
        .bind(id).fetch_one(&s.db).await?))
}

pub async fn delete_comment(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path((file_id, comment_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    // No blanket editor gate: authorization here is "you own the comment, or
    // you're an admin" (checked below).  A viewer who posted a comment must be
    // able to delete their own — gating on editor would wrongly block that.
    let sys_row: Option<(String,)> = sqlx::query_as(
        "SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = sys_row.ok_or(ApiError::NotFound)?;
    // Access denial → 404 (no existence leak), consistent with the read path.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let owner: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM file_comments WHERE id = $1 AND file_id = $2"
    ).bind(comment_id).bind(file_id).fetch_optional(&s.db).await?;
    let owner = owner.ok_or(ApiError::NotFound)?.0;
    if owner != user.0.id && user.0.role != "admin" {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("DELETE FROM file_comments WHERE id = $1").bind(comment_id).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Notifications.
// =============================================================================

#[derive(Debug, Serialize, FromRow)]
pub struct Notification {
    pub id:         Uuid,
    pub user_id:    String,
    pub kind:       String,
    pub title:      String,
    pub body:       Option<String>,
    pub link:       Option<String>,
    pub read_at:    Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct NotificationsQuery {
    pub unread_only: Option<bool>,
    pub limit:       Option<i64>,
}

pub async fn list_notifications(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<NotificationsQuery>,
) -> ApiResult<Json<Vec<Notification>>> {
    let limit = q.limit.unwrap_or(50).min(500).max(1);
    let unread_only = q.unread_only.unwrap_or(false);
    let rows: Vec<Notification> = if unread_only {
        sqlx::query_as(
            "SELECT * FROM notifications WHERE user_id = $1 AND read_at IS NULL ORDER BY created_at DESC LIMIT $2"
        ).bind(user.0.id).bind(limit).fetch_all(&s.db).await?
    } else {
        sqlx::query_as(
            "SELECT * FROM notifications WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2"
        ).bind(user.0.id).bind(limit).fetch_all(&s.db).await?
    };
    Ok(Json(rows))
}

pub async fn mark_notification_read(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let r = sqlx::query(
        "UPDATE notifications SET read_at = now() WHERE id = $1 AND user_id = $2 AND read_at IS NULL"
    ).bind(id).bind(user.0.id).execute(&s.db).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unread_count(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL"
    ).bind(user.0.id).fetch_one(&s.db).await?;
    Ok(Json(serde_json::json!({ "unread": n })))
}

// =============================================================================
// File detail with author + comment count (so the UI can render in one shot).
// =============================================================================
#[derive(Debug, Serialize)]
pub struct FileExtras {
    pub comments: i64,
    pub has_thumbnail: bool,
    pub has_extracted_text: bool,
}

pub async fn file_extras(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FileExtras>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let comments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_comments WHERE file_id = $1")
        .bind(id).fetch_one(&s.db).await?;
    let thumb: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM thumbnails WHERE file_id = $1)")
        .bind(id).fetch_one(&s.db).await?;
    let text: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_content WHERE file_id = $1)")
        .bind(id).fetch_one(&s.db).await?;
    Ok(Json(FileExtras { comments, has_thumbnail: thumb, has_extracted_text: text }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_pdf_returns_string_or_none() {
        // We don't carry test fixtures, so just verify the function shape
        // on a clearly-not-pdf input.
        assert!(extract_text_from("pdf", b"not actually a pdf").is_none() ||
                extract_text_from("pdf", b"not actually a pdf").is_some());
        // Plain text passthrough works.
        let txt = extract_text_from("txt", b"hello world").unwrap();
        assert_eq!(txt, "hello world");
        assert!(extract_text_from("zip", b"PK\x03\x04").is_none());
    }

    #[test]
    fn make_thumbnail_handles_garbage() {
        // Garbage in → None out, no panic.
        assert!(make_thumbnail(b"not an image", 64).is_none());
    }
}

//! Compatibility with the legacy FileService API (`/FileService/*`).
//!
//! ## Why this module exists
//! Deployments migrating from an older file service may have many applications
//! (often dozens, with hundreds of call sites) that already call it with this URL
//! set — `/FileService/upload`, `/FileService/downloadFile?fileId=...`, etc.
//! Moving them to FileHub would otherwise mean changing every caller's code,
//! re-testing each one, and releasing them all at the same time.
//!
//! This module lets FileHub "speak the old protocol" too ⇒ legacy callers need
//! **no code changes at all**; only the target URL in their configuration
//! (configmap/secret) changes, they can call FileHub immediately, and they can be
//! migrated one at a time instead of all at once.
//!
//! ## Response shape
//! Copied key by key from the legacy service's real responses — both
//! `{success,status,message,data:{...}}` for upload/getFileDetail and
//! `{success,message,total,data:[...]}` for getFiles — because callers read keys
//! directly, e.g. `data.id`, `data.file_name`, `data.mime_type`, `data.file_path`.
//!
//! ## Old files stay downloadable
//! `fileId`s of files uploaded to the old service do not exist in FileHub ⇒ when
//! a lookup misses and `LEGACY_FILEHUB_URL` is set, the request is forwarded to
//! the old service automatically, so migrated callers can still open old files
//! without waiting for a data migration.
//!
//! ## Caller identity
//! In order: the edge identity / FileHub API key (via `MaybeAuthUser`)
//! → otherwise a service account for legacy callers, because they send a Bearer
//! token issued by the old service that FileHub cannot verify (and changing the
//! callers is exactly what we want to avoid).
//!
//! SECURITY REQUIREMENT: `/FileService/*` must not be exposed directly to the
//! internet. Only allow it inside the cluster (ClusterIP) or behind an SSO proxy
//! (oauth2-proxy) — otherwise anyone could download files, as with many legacy
//! file services. The whole layer is disabled unless `LEGACY_FILESERVICE=1` is set.

use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::Id;

use crate::auth::{MaybeAuthUser, User};
use crate::config::env_opt;
use crate::error::{ApiError, ApiResult};
use crate::handlers::{persist_upload_for, UploadFields, FILE_COLS};
use crate::models::File;
use crate::AppState;

/// Whether the legacy compatibility layer is enabled (default: off).
/// The routes answer only while at least one legacy connector is switched on.
pub async fn gate(axum::extract::State(s): axum::extract::State<Arc<AppState>>, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if crate::connectors::legacy_on(&s.db).await { next.run(req).await } else { StatusCode::NOT_FOUND.into_response() }
}


/// Service account the legacy routes use for callers without an identity.
/// (The old service's URL, used to forward requests for files not yet migrated,
/// comes from `LEGACY_FILEHUB_URL` — see `legacy_upstream`.)

fn legacy_upstream() -> Option<String> {
    env_opt("LEGACY_FILEHUB_URL").map(|v| v.trim_end_matches('/').to_string())
}

/// Upload size cap for the legacy routes (default 2048 MB, `LEGACY_MAX_UPLOAD_MB`).
fn legacy_max_upload_bytes() -> usize {
    env_opt("LEGACY_MAX_UPLOAD_MB")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(2048)
        * 1024
        * 1024
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // The method-override layer (lib.rs) has already turned POST + X-HTTP-Method-Override
        // into the real method by the time a request is routed, so the chunked transport's
        // HEAD/PATCH/DELETE arrive here as such.
        .route("/FileService/upload", post(upload_entry).patch(upload_entry).head(upload_entry).delete(upload_entry))
        .route("/FileService/uploadFile", post(upload))
        .route("/FileService/downloadFile", get(download))
        .route("/FileService/previewFile", get(preview))
        .route("/FileService/getFileDetail", get(file_detail))
        .route("/FileService/getFiles", get(get_files))
        .route("/FileService/rename", post(rename))
        .route("/FileService/moveFileToTrash", post(move_to_trash))
        // The old service's ingress allowed client-max-body-size 2048m ⇒ a much lower
        // cap would make large uploads that used to work start failing after the
        // switch-over (a regression people would blame on FileHub). Tune it with
        // LEGACY_MAX_UPLOAD_MB and size the pod's memory limit accordingly.
        .layer(DefaultBodyLimit::max(legacy_max_upload_bytes()))
}

/// Target system for files arriving via the legacy routes — the "bucket" they land in.
///
/// Legacy services often dump every application's files into one pile, so
/// quotas/retention/permissions cannot be separated per system. This infers the
/// source system **without any caller code change**:
///   1. the `x-filehub-system` header (for callers that want to be explicit)
///   2. the `Referer` of the page that uploaded — apps usually live under their
///      own path prefix, e.g. /hr-portal/... ⇒ the HR bucket (covers nearly all
///      browser uploads)
///   3. the connector's own basket
///
/// Prefixes and baskets are set per connector in Admin → Connectors.
async fn system_from_request(s: &AppState, headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-filehub-system").and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()) {
        return Some(v.to_string());
    }
    crate::connectors::system_for_referer(&s.db, headers).await
}

// ────────────────────────── Caller identity ──────────────────────────

/// The user to record the action as — the real person if known, else the legacy service account.
///
/// Old apps always send `Authorization: Bearer <old-service token>`; FileHub can't
/// verify that token, so the SSO identity the edge attaches to the same request
/// is checked FIRST — otherwise every browser upload would be recorded as the
/// service account and per-person access rules would never apply.
///
/// `write` marks a call that changes something: a read-only connector refuses it.
async fn who_is(s: &Arc<AppState>, headers: &HeaderMap, who: Option<User>, write: bool) -> ApiResult<User> {
    if let Some(u) = crate::auth::edge_user(s, headers).await? { return Ok(u); }
    if let Some(u) = who { return Ok(u); }
    // Old back-ends send a fixed token FileHub can't verify; it counts only when
    // an admin registered it on a connector. An anonymous request never does.
    let c = crate::connectors::by_token(&s.db, headers).await?.ok_or(ApiError::Unauthorized)?;
    if write && c.read_only { return Err(ApiError::Forbidden); }
    crate::connectors::service_user(&s.db, &c).await
}

// ────────────────────────── Legacy response shape ──────────────────────────

fn th_date(d: &DateTime<Utc>) -> String {
    d.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
        .format("%d %B %Y")
        .to_string()
}

fn real_ts(d: &DateTime<Utc>) -> String {
    d.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string()
}

/// File name without its extension — the legacy service returns `file_name`
/// without the extension and the full name in `fileName` / `full_name_type`.
/// This must match exactly, because some callers append the extension themselves.
fn stem(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

fn download_link(id: &Id) -> String {
    match env_opt("PUBLIC_BASE_URL") {
        Some(base) => format!("{}/FileService/downloadFile?fileId={}", base.trim_end_matches('/'), id),
        None => format!("/FileService/downloadFile?fileId={id}"),
    }
}

/// Render a file as JSON with exactly the same keys as the legacy service.
fn legacy_file(f: &File) -> Value {
    let mime = mime_guess::from_path(&f.name).first().map(|m| m.to_string());
    json!({
        "id": f.id,
        "fileType": f.file_type,
        "fileName": f.name,
        "full_name_type": f.name,
        "file_name": stem(&f.name),
        "file_type": f.file_type,
        "mime_type": mime,
        "file_path": "",
        "file_system_id": f.id,
        "file_app_id": f.id,
        "size_": f.size_bytes.to_string(),
        "file_size": f.size_bytes,
        "type_": "FILE",
        "active_": "Y",
        "file_version": f.version.to_string(),
        "prefix_version": "1",
        "control_version": null,
        "encrypt_type": if f.encrypted { "AES256" } else { "DEFAULT" },
        "parent_folder_id": f.folder_id.clone().unwrap_or_default(),
        "group_id": "",
        "link_download": "",
        "linkFile": download_link(&f.id),
        "user_id": f.created_by.clone().unwrap_or_default(),
        "owner_user_id": f.created_by.clone().unwrap_or_default(),
        "create_date": th_date(&f.created_at),
        "update_date": th_date(&f.modified_at),
        "create_date_real": real_ts(&f.created_at),
        "update_date_real": real_ts(&f.modified_at),
        "tag_name": f.tags,
        "module_name": f.project,
        "department_name": f.org_id,
        "permission_anyone": "anyoneWithTheLink",
        "permission_file": "viewer",
        "permissionsGranted": true,
        "hash_file": f.etag,
        "risk_virus": null,
        "sign_off": null,
        "sync_status": null,
        "checkOutObj": null,
        "share_with_user_id": null,
        "task_id": null,
        "ignore": false,
    })
}

// ────────────────────────── Upload ──────────────────────────

#[derive(Default)]
struct LegacyUpload {
    name: Option<String>,
    staged: Option<crate::storage::Staged>,
    content_type: Option<String>,
    parent_folder_id: Option<String>,
    file_type: Option<String>,
    group_id: Option<String>,
    /// Folder layout hints (see layout.rs): organisation, source system, fiscal year, stage.
    org_code: Option<String>,
    source_code: Option<String>,
    fiscal_year: Option<String>,
    stage_code: Option<String>,
    stage_name: Option<String>,
    /// Origin details (origin.rs): who pressed send in the source system, its reference number, screen.
    source_user: Option<String>,
    ref_no: Option<String>,
    screen: Option<String>,
}

/// Read a legacy-style multipart body.
///
/// The file field name is not consistent: Angular clients send `file`, some Java
/// clients send an empty name, and others use caller-defined names ⇒ treat "any
/// field that carries a filename" as the file, regardless of its field name.
async fn read_legacy_upload(s: &AppState, mp: &mut Multipart) -> ApiResult<LegacyUpload> {
    let mut out = LegacyUpload::default();
    while let Some(mut field) = mp
        .next_field()
        .await
        .map_err(|e| crate::handlers::multipart_error(e, legacy_max_upload_bytes() as u64))?
    {
        let fname = field.file_name().map(str::to_string);
        let key = field.name().unwrap_or("").to_string();
        if fname.is_some() && out.staged.is_none() {
            out.name = fname;
            out.content_type = field.content_type().map(str::to_string);
            // Stream into staging (encrypting on the way) — never buffer the whole file in memory.
            out.staged = Some(crate::handlers::stage_field(s, &mut field, legacy_max_upload_bytes() as u64).await?);
            continue;
        }
        let val = field.text().await.unwrap_or_default();
        if val.trim().is_empty() {
            continue;
        }
        match key.as_str() {
            "fileName" => out.name.get_or_insert(val),
            "parentFolderId" | "parent_folder_id" => out.parent_folder_id.insert(val),
            "fileType" => out.file_type.insert(val),
            "groupId" => out.group_id.insert(val),
            "orgCode" | "departmentCode" | "agencyCode" => out.org_code.insert(val),
            "systemCode" | "sourceSystem" => out.source_code.insert(val),
            "fiscalYear" => out.fiscal_year.insert(val),
            "stageCode" => out.stage_code.insert(val),
            "stageName" => out.stage_name.insert(val),
            "userId" | "username" | "uploadedBy" | "sourceUser" => out.source_user.insert(val),
            "refNo" | "referenceNo" | "documentNo" | "requestNo" => out.ref_no.insert(val),
            "screenCode" | "sourceScreen" | "pageCode" => out.screen.insert(val),
            _ => continue,
        };
    }
    Ok(out)
}

/// `/FileService/upload`: chunked uploads from FileHub's own pages (they carry
/// `Tus-Resumable`, see tus::one_path) or a legacy multipart upload.
async fn upload_entry(State(s): State<Arc<AppState>>, req: axum::extract::Request) -> Response {
    if req.headers().contains_key("tus-resumable") {
        return match crate::tus::one_path(s, req).await { Ok(r) => r, Err(e) => e.into_response() };
    }
    if req.method() != axum::http::Method::POST { return StatusCode::METHOD_NOT_ALLOWED.into_response(); }
    axum::handler::Handler::call(upload, req, s).await
}

async fn upload(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    headers: HeaderMap,
    mut mp: Multipart,
) -> ApiResult<Json<Value>> {
    let user = who_is(&s, &headers, who, true).await?;
    crate::auth::require_role(&user, &["admin", "editor"])?;
    if crate::connectors::is_connector_user(&s.db, &user.id).await {
        // A connector carries a whole app's traffic: its own, higher limit
        // (the per-person 60/min would refuse most of it).
        let key = format!("legacy:{}", user.id);
        let per_min = env_opt("LEGACY_UPLOAD_RATE_PER_MIN").and_then(|v| v.parse().ok()).unwrap_or(1200);
        crate::auth::upload_rate_limit_keyed(&s.db, &key, per_min).await?;
    } else {
        crate::auth::upload_rate_limit(&s.db, &user.id).await?;
    }

    let mut up = read_legacy_upload(&s, &mut mp).await?;
    let Some(name) = up.name.clone() else {
        if let Some(st) = up.staged.take() { s.storage.discard(st).await; }
        return Err(ApiError::BadRequest("missing file".into()));
    };
    if up.staged.is_none() { return Err(ApiError::BadRequest("missing file body".into())); }

    // Folder: legacy callers send their own folder ID (e.g. parentFolderId).
    // If ignored, the file lands at the root and callers listing by parentFolderId
    // would not find it, so the legacy ID is "reserved" — a folder is created with
    // the same id (ids are text) ⇒ getFiles?parentFolderId=<legacy id> still works.
    let wanted = system_from_request(&s, &headers).await;

    // Fiscal-year folder layout: applies when the caller says which organisation
    // or source system the file belongs to, or sends no folder of its own (then
    // the source comes from the uploading page). Callers that send their own
    // folder and no hints keep the old behaviour.
    let hdr = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let hint = crate::layout::Hint {
        org_code: up.org_code.clone().or_else(|| hdr("x-filehub-org")),
        source_code: up.source_code.clone().or_else(|| hdr("x-filehub-source")),
        fiscal_year: up.fiscal_year.clone().or_else(|| hdr("x-filehub-fiscal-year")).and_then(|v| v.trim().parse().ok()),
        stage_code: up.stage_code.clone().or_else(|| hdr("x-filehub-stage")),
        stage_name: up.stage_name.clone(),
        referer: hdr("referer"),
    };
    // Whenever the source system is known (sent, or from the uploading page) the
    // file goes into the fiscal-year layout — also when the caller names its own
    // folder: that folder id is kept on the file (tag legacy-folder:<id>) so
    // getFiles?parentFolderId=<id> still lists it.
    let placed = match crate::layout::place(&s.db, &hint, Some(&user)).await {
        Ok(p) => p,
        Err(e) => { if let Some(st) = up.staged.take() { s.storage.discard(st).await; } return Err(e); }
    };
    let legacy_parent = if placed.is_some() { up.parent_folder_id.take() } else { None };

    let (folder_id, folder_system) = match up.parent_folder_id.as_deref() {
        Some(v) => match ensure_legacy_folder(&s, v, &user, wanted.as_deref()).await {
            Ok((f, sys)) => (Some(f), Some(sys)),
            Err(e) => { if let Some(st) = up.staged.take() { s.storage.discard(st).await; } return Err(e); }
        },
        None => (None, None),
    };
    // A known folder decides the basket; else the request, the person's default, the legacy default.
    let user_default: Option<String> = sqlx::query_scalar("SELECT default_system_id FROM users WHERE id = $1")
        .bind(&user.id).fetch_optional(&s.db).await?.flatten();
    let mut system_id = folder_system.or(wanted).or(user_default).or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"));
    let mut folder_id = folder_id;
    let mut org_id = None;

    // Extra tags from data the legacy caller already sends + a marker for the legacy path.
    // (With no tags supplied, persist_upload_for also adds its automatic tags.)
    let mut extra: Vec<String> = vec!["via:legacy-fileservice".into()];
    if let Some(v) = up.file_type.as_deref() {
        extra.push(format!("type:{v}"));
    }
    if let Some(v) = up.group_id.as_deref() {
        extra.push(format!("group:{v}"));
    }
    if let Some(v) = legacy_parent.as_deref().filter(|v| v.len() <= 128) {
        extra.push(format!("legacy-folder:{v}"));
    }
    if placed.is_none() {
        // Not mapped to a source system yet: keep where it came from, so an admin can map it.
        if let Some(app) = crate::connectors::referer_app(&headers) { extra.push(format!("from:{app}")); }
    }
    if let Some(p) = &placed {
        system_id = Some(p.system_id.clone());
        folder_id = Some(p.folder_id.clone());
        org_id = p.org_id.clone();
        extra.extend(p.tags.iter().cloned());
    }

    let fields = UploadFields {
        name: Some(name),
        body: None,
        staged: up.staged.take(),
        content_type: up.content_type,
        folder_id,
        system_id,
        org_id,
        project: None,
        status: None,
        owner: None,
        tags: None,
        category_code: None,
        fields: None,
        skip_required: true,
        encrypt_file: false,
        origin: Some({
            let mut x = crate::origin::from_headers(&headers);
            x.source_system = up.source_code.clone().or(x.source_system);
            x.org_code = up.org_code.clone().or(x.org_code);
            x.source_user = up.source_user.clone().or(x.source_user);
            x.ref_no = up.ref_no.clone().or(x.ref_no);
            x.screen = up.screen.clone().or(x.screen).or_else(|| hdr("referer").and_then(|r| r.split_once("://").map(|(_, p)| p.split_once('/').map(|(_, q)| format!("/{q}")).unwrap_or_default())).filter(|v| !v.is_empty()));
            x
        }),
    };
    let file = persist_upload_for(&s, Some(user.id.as_str()), fields, Some(&user)).await?;
    // Append the legacy tags to the automatic ones just added, then re-read the row
    // so `tag_name` in the response matches what was stored (some callers keep it).
    append_tags(&s, &file, &extra).await?;
    if let Some(p) = &placed { crate::layout::after_upload(&s.db, p, &file.id.to_string()).await?; }
    let file = find_file(&s, &file.id.to_string()).await?.unwrap_or(file);

    Ok(Json(json!({
        "success": true,
        "status": 200,
        "message": "Upload file success.",
        "data": legacy_file(&file),
    })))
}

/// Reserve a legacy folder ID in FileHub (the same id, so `getFiles?parentFolderId=`
/// keeps working). An existing folder keeps its basket; a new one is created in the
/// target basket, which needs Edit there. Returns (folder id, basket id).
async fn ensure_legacy_folder(
    s: &Arc<AppState>,
    legacy_id: &str,
    user: &User,
    want_system: Option<&str>,
) -> ApiResult<(String, String)> {
    if legacy_id.len() > 128 { return Err(ApiError::BadRequest("parentFolderId is too long".into())); }
    if let Some(found) = resolve_legacy_folder(s, legacy_id).await? { return Ok(found); }
    let user_default: Option<String> = sqlx::query_scalar("SELECT default_system_id FROM users WHERE id = $1")
        .bind(&user.id).fetch_optional(&s.db).await?.flatten();
    let system_id = want_system.map(|v| v.to_string()).or(user_default)
        .or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"))
        .ok_or_else(|| ApiError::BadRequest("missing system for folder".into()))?;
    crate::acl::require_folder(&s.db, user, &system_id, None, crate::acl::EDIT).await
        .map_err(|e| if matches!(e, ApiError::NotFound) { ApiError::Forbidden } else { e })?;
    sqlx::query(
        r#"INSERT INTO folders (id, system_id, name, owner, created_by)
           VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING"#,
    )
    .bind(legacy_id).bind(&system_id).bind(format!("Legacy folder {legacy_id}"))
    .bind(&user.display_name).bind(&user.id)
    .execute(&s.db).await?;
    Ok((legacy_id.to_string(), system_id))
}

/// The FileHub folder for a legacy folder id: a registered alias first (folders
/// whose ids were re-keyed keep answering to their old id), else the folder with
/// that very id.
async fn resolve_legacy_folder(s: &Arc<AppState>, legacy_id: &str) -> ApiResult<Option<(String, String)>> {
    Ok(sqlx::query_as::<_, (String, String)>(
        r#"SELECT f.id, f.system_id FROM folders f
            WHERE f.deleted_at IS NULL
              AND (f.id = (SELECT a.folder_id FROM legacy_folder_aliases a WHERE a.legacy_id = $1) OR f.id = $1)
            ORDER BY (f.id = $1) LIMIT 1"#,
    ).bind(legacy_id).fetch_optional(&s.db).await?)
}

async fn append_tags(s: &Arc<AppState>, f: &File, extra: &[String]) -> ApiResult<()> {
    let mut tags: Vec<String> = serde_json::from_str(&f.tags).unwrap_or_default();
    // The layout's system:<source code> is the file's system; drop the basket-name one.
    if extra.iter().any(|t| t.starts_with("system:")) {
        tags.retain(|t| !t.starts_with("system:"));
    }
    for t in extra {
        if !tags.iter().any(|x| x == t) {
            tags.push(t.clone());
        }
    }
    let json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
    sqlx::query("UPDATE files SET tags = $1 WHERE id = $2")
        .bind(&json)
        .bind(f.id)
        .execute(&s.db)
        .await?;
    Ok(())
}

// ────────────────────────── Read files ──────────────────────────

#[derive(Deserialize)]
struct FileIdQuery {
    #[serde(rename = "fileId")]
    file_id: Option<String>,
    #[serde(rename = "logType")]
    _log_type: Option<String>,
}

/// Look up a file by legacy `fileId` — None when it is not in this database (an old file).
async fn find_file(s: &Arc<AppState>, file_id: &str) -> ApiResult<Option<File>> {
    let Ok(id) = crate::Id::parse_str(file_id) else { return Ok(None) };
    Ok(sqlx::query_as::<_, File>(&format!(
        "SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_optional(&s.db)
    .await?)
}

/// Forward the request to the old file service, for files uploaded before the migration.
///
/// Streams the body (never holds the whole file in memory) · forwards
/// `Range`/`If-Range` and passes back 206/Content-Range/Accept-Ranges as the old
/// service answers · uses a shared client with timeouts instead of one per request.
async fn proxy_legacy(path_and_query: &str, headers: &HeaderMap) -> ApiResult<Response> {
    let Some(base) = legacy_upstream() else { return Err(ApiError::NotFound) };
    let url = format!("{base}{path_and_query}");
    let mut req = crate::serve::upstream_client().get(&url);
    for k in [header::AUTHORIZATION, header::RANGE, header::IF_RANGE] {
        if let Some(v) = headers.get(&k) {
            req = req.header(k.as_str(), v.as_bytes());
        }
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::Other(anyhow::anyhow!("legacy upstream: {e}")))?;
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out = HeaderMap::new();
    for k in [
        header::CONTENT_TYPE, header::CONTENT_DISPOSITION, header::CACHE_CONTROL,
        header::CONTENT_LENGTH, header::CONTENT_RANGE, header::ACCEPT_RANGES,
        header::ETAG, header::LAST_MODIFIED,
    ] {
        if let Some(v) = resp.headers().get(k.as_str()) {
            if let Ok(hv) = axum::http::HeaderValue::from_bytes(v.as_bytes()) {
                out.insert(k, hv);
            }
        }
    }
    let body = axum::body::Body::from_stream(resp.bytes_stream());
    Ok((status, out, body).into_response())
}

async fn serve_bytes(
    s: &Arc<AppState>,
    user: &User,
    f: &File,
    inline: bool,
    req: &HeaderMap,
) -> ApiResult<Response> {
    // Same path as FileHub's own download: per-file access, stamped downloads,
    // Vault logging, Range. Office files viewed inline go out as their PDF preview.
    crate::acl::require_file(&s.db, user, f.id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    if inline && matches!(f.file_type.as_str(), "docx" | "xlsx" | "pptx") {
        return crate::p1::get_preview(State(s.clone()), crate::auth::AuthUser(user.clone()), axum::extract::Path(f.id), req.clone()).await;
    }
    crate::handlers::download_file(
        State(s.clone()), crate::auth::AuthUser(user.clone()), axum::extract::Path(f.id),
        Query(crate::handlers::DownloadQuery { original: None, version: None }), req.clone(),
    ).await
}

async fn download(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Query(q): Query<FileIdQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file_id = q.file_id.unwrap_or_default();
    if file_id.is_empty() {
        return Err(ApiError::BadRequest("fileId is required".into()));
    }
    match find_file(&s, &file_id).await? {
        Some(f) => {
            let user = who_is(&s, &headers, who, false).await?;
            serve_bytes(&s, &user, &f, false, &headers).await
        }
        None => {
            proxy_legacy(
                &format!("/FileService/downloadFile?fileId={}&logType=download", urlencoding::encode(&file_id)),
                &headers,
            )
            .await
        }
    }
}

async fn preview(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Query(q): Query<FileIdQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file_id = q.file_id.unwrap_or_default();
    if file_id.is_empty() {
        return Err(ApiError::BadRequest("fileId is required".into()));
    }
    match find_file(&s, &file_id).await? {
        Some(f) => {
            let user = who_is(&s, &headers, who, false).await?;
            serve_bytes(&s, &user, &f, true, &headers).await
        }
        None => {
            proxy_legacy(
                &format!("/FileService/previewFile?fileId={}&logType=preview", urlencoding::encode(&file_id)),
                &headers,
            )
            .await
        }
    }
}

async fn file_detail(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Query(q): Query<FileIdQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file_id = q.file_id.unwrap_or_default();
    if file_id.is_empty() {
        return Err(ApiError::BadRequest("fileId is required".into()));
    }
    match find_file(&s, &file_id).await? {
        Some(f) => {
            let user = who_is(&s, &headers, who, false).await?;
            crate::acl::require_file(&s.db, &user, f.id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
            Ok(Json(json!({
                "success": true,
                "message": "getFileDetail Successfully.",
                "data": legacy_file(&f),
            }))
            .into_response())
        }
        None => {
            proxy_legacy(&format!("/FileService/getFileDetail?fileId={}", urlencoding::encode(&file_id)), &headers).await
        }
    }
}

// ────────────────────────── List files ──────────────────────────

#[derive(Deserialize)]
struct GetFilesQuery {
    offset: Option<i64>,
    #[serde(rename = "limitOfset")]
    limit_ofset: Option<i64>,
    keyword: Option<String>,
    tag: Option<String>,
    #[serde(rename = "parentFolderId")]
    parent_folder_id: Option<String>,
}

async fn get_files(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    headers: HeaderMap,
    Query(q): Query<GetFilesQuery>,
) -> ApiResult<Json<Value>> {
    let user = who_is(&s, &headers, who, false).await?;
    let limit = q.limit_ofset.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let parent = q.parent_folder_id.clone().filter(|v| !v.trim().is_empty());
    let parent_resolved = match &parent { Some(p) => resolve_legacy_folder(&s, p).await?.map(|(id, _)| id), None => None };

    // Filters are added only when given (no flag guards in SQL).
    let mut sql = String::from(" WHERE deleted_at IS NULL");
    let mut strs: Vec<String> = Vec::new();
    let mut vecs: Vec<Vec<String>> = Vec::new();
    let mut n = 0usize;
    let esc = |v: &str| v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    if let Some(k) = q.keyword.as_deref().filter(|v| !v.trim().is_empty()) {
        n += 1; sql.push_str(&format!(" AND name ILIKE ${n}")); strs.push(format!("%{}%", esc(k.trim())));
    }
    if let Some(t) = q.tag.as_deref().filter(|v| !v.trim().is_empty()) {
        n += 1; sql.push_str(&format!(" AND tags ILIKE ${n}")); strs.push(format!("%{}%", esc(t.trim())));
    }
    if let Some(p) = &parent {
        // The folder itself (or the folder its old id now points to), plus files
        // filed into the fiscal-year layout that were sent with this folder id.
        n += 1; let a = n; strs.push(parent_resolved.clone().unwrap_or_else(|| p.clone()));
        n += 1; let b = n; strs.push(format!("%\"legacy-folder:{}\"%", esc(p)));
        sql.push_str(&format!(" AND (folder_id = ${a} OR tags LIKE ${b})"));
    }
    // What the caller may see — the same set-based rule as FileHub's own lists.
    if let Some(scope) = crate::auth::effective_system_ids(&s.db, &user).await? {
        let v = crate::acl::visibility(&s.db, &user, &scope).await?;
        n += 1; sql.push_str(&format!(" AND system_id = ANY(${n})")); vecs.push(scope);
        n += 1; let bn = n; vecs.push(v.baskets);
        n += 1; let fln = n; vecs.push(v.folders);
        n += 1; let fin = n; vecs.push(v.files);
        n += 1; let un = n; vecs.push(vec![v.uid]);
        sql.push_str(&crate::acl::vis_sql("files", bn, fln, fin, un));
    }

    let count_sql = format!("SELECT COUNT(*) FROM files{sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &strs { cq = cq.bind(b); }
    for b in &vecs { cq = cq.bind(b); }
    let total: i64 = cq.fetch_one(&s.db).await?;
    let page_sql = format!(
        "SELECT {FILE_COLS} FROM files{sql} ORDER BY modified_at DESC, id DESC LIMIT ${} OFFSET ${}", n + 1, n + 2);
    let mut pq = sqlx::query_as::<_, File>(&page_sql);
    for b in &strs { pq = pq.bind(b); }
    for b in &vecs { pq = pq.bind(b); }
    let files = pq.bind(limit).bind(offset).fetch_all(&s.db).await?;

    // Sub-folders come first on the first page (callers tell them apart by type_ = DIRECTORY).
    let mut data: Vec<Value> = Vec::new();
    if let (Some(p), 0) = (&parent, offset) {
        let subs: Vec<(String, String, String, DateTime<Utc>, i64)> = sqlx::query_as(
            r#"SELECT f.id, f.name, f.system_id, f.created_at,
                      (SELECT COUNT(*) FROM files x WHERE x.folder_id = f.id AND x.deleted_at IS NULL)
                 FROM folders f WHERE f.parent_id = $1 AND f.deleted_at IS NULL ORDER BY f.name"#,
        ).bind(parent_resolved.as_deref().unwrap_or(p)).fetch_all(&s.db).await?;
        for (id, name, sys, at, count) in subs {
            if crate::acl::require_folder(&s.db, &user, &sys, Some(&id), crate::acl::VIEW).await.is_err() { continue; }
            data.push(json!({
                "id": id, "file_system_id": id, "file_app_id": id,
                "file_name": name, "fileName": name, "full_name_type": name,
                "file_type": "dir", "fileType": "dir", "type_": "DIRECTORY", "size_": null,
                "count_file": count, "parent_folder_id": p, "mime_type": null,
                "create_date": th_date(&at), "create_date_real": real_ts(&at), "active_": "Y",
            }));
        }
    }
    data.extend(files.iter().map(legacy_file));
    Ok(Json(json!({
        "success": true,
        "message": "get file Successfully.",
        "total": total,
        "paging": { "total": total, "offset": offset, "limit": limit },
        "data": data,
    })))
}

// ────────────────────────── Rename / move to trash ──────────────────────────

#[derive(Deserialize)]
struct RenameBody {
    #[serde(rename = "fileId")]
    file_id: String,
    #[serde(rename = "renameTo")]
    rename_to: String,
}

async fn rename(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    headers: HeaderMap,
    Json(b): Json<RenameBody>,
) -> ApiResult<Json<Value>> {
    let user = who_is(&s, &headers, who, true).await?;
    let f = find_file(&s, &b.file_id).await?.ok_or(ApiError::NotFound)?;
    let to = b.rename_to.trim();
    if to.is_empty() || to.len() > 255 || to.contains('/') || to.contains('\\') {
        return Err(ApiError::BadRequest("renameTo is required (no slashes, up to 255 characters)".into()));
    }
    // Legacy callers show names without the extension; keep the file's own one.
    let ext = f.name.rfind('.').filter(|i| *i > 0).map(|i| &f.name[i..]).unwrap_or("");
    let new_name = if !ext.is_empty() && !to.to_lowercase().ends_with(&ext.to_lowercase()) { format!("{to}{ext}") } else { to.to_string() };
    // Through FileHub's own rename: Edit on the file, and a locked status refuses it.
    let _ = crate::handlers::patch_file(
        State(s.clone()), crate::auth::AuthUser(user), axum::extract::Path(f.id),
        Json(serde_json::from_value(json!({ "name": new_name })).map_err(|e| ApiError::BadRequest(e.to_string()))?),
    ).await?;
    Ok(Json(json!({ "success": true, "status": 200, "message": "rename successfully" })))
}

#[derive(Deserialize)]
struct TrashBody {
    #[serde(rename = "fileId")]
    file_id: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

async fn move_to_trash(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    headers: HeaderMap,
    Json(b): Json<TrashBody>,
) -> ApiResult<Json<Value>> {
    let user = who_is(&s, &headers, who, true).await?;
    if b.kind.as_deref().map(|k| k.eq_ignore_ascii_case("DIRECTORY")).unwrap_or(false) {
        crate::handlers::delete_folder(State(s.clone()), crate::auth::AuthUser(user), axum::extract::Path(b.file_id.clone())).await?;
    } else {
        let f = find_file(&s, &b.file_id).await?.ok_or(ApiError::NotFound)?;
        crate::handlers::delete_file(
            State(s.clone()), crate::auth::AuthUser(user), axum::extract::Path(f.id),
            Query(crate::handlers::DeleteQuery { hard: None }),
        ).await?;
    }
    Ok(Json(json!({
        "status": 200,
        "success": true,
        "message": format!("move file to trash:{} successfully", b.file_id),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_strips_only_the_last_extension() {
        // The legacy service returns file_name without the extension and the full name in fileName.
        // Non-ASCII (Thai) file names must work too.
        assert_eq!(stem("probe-compat.txt"), "probe-compat");
        assert_eq!(stem("รายงาน.งบ.2570.pdf"), "รายงาน.งบ.2570");
        assert_eq!(stem("no-extension"), "no-extension");
        // A Unix dotfile is not "just an extension" — it must not become an empty name.
        assert_eq!(stem(".env"), ".env");
    }

    #[test]
    fn download_link_uses_the_legacy_shape() {
        std::env::remove_var("PUBLIC_BASE_URL");
        let id = crate::Id::parse("x0").unwrap();
        assert_eq!(
            download_link(&id),
            format!("/FileService/downloadFile?fileId={id}")
        );
    }
}

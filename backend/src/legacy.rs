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
use uuid::Uuid;

use crate::auth::{MaybeAuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::handlers::{persist_upload_for, UploadFields, FILE_COLS};
use crate::models::File;
use crate::AppState;

/// Whether the legacy compatibility layer is enabled (default: off).
pub fn enabled() -> bool {
    matches!(
        std::env::var("LEGACY_FILESERVICE").unwrap_or_default().trim(),
        "1" | "true" | "on" | "yes"
    )
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Service account the legacy routes use for callers without an identity.
/// (The old service's URL, used to forward requests for files not yet migrated,
/// comes from `LEGACY_FILEHUB_URL` — see `legacy_upstream`.)
const LEGACY_SERVICE_USER_ID: &str = "usr_legacy_fileservice";

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
        .route("/FileService/upload", post(upload))
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
///   3. the `LEGACY_DEFAULT_SYSTEM` default
///
/// The mapping is set in `LEGACY_SYSTEM_MAP`, e.g. "hr-portal=sys_hr,e-sign=sys_esign"
/// (comma-separated) — new apps can be added later without a rebuild.
fn system_from_request(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-filehub-system").and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !referer.is_empty() {
        for pair in env_opt("LEGACY_SYSTEM_MAP").unwrap_or_default().split(',') {
            let (prefix, system) = match pair.split_once('=') {
                Some((a, b)) => (a.trim().to_lowercase(), b.trim().to_string()),
                None => continue,
            };
            if prefix.is_empty() || system.is_empty() {
                continue;
            }
            // Match path segments only, so a host name cannot match by accident.
            if referer.contains(&format!("/{prefix}/")) || referer.ends_with(&format!("/{prefix}")) {
                return Some(system);
            }
        }
    }
    None
}

// ────────────────────────── Caller identity ──────────────────────────

/// Service account for legacy callers — created once, then reused.
///
/// Legacy callers send `Authorization: Bearer <token of the old service>`, which
/// FileHub cannot verify (different identity system), and changing the callers
/// defeats the purpose. So requests that reach these routes are accepted as
/// "legacy" and recorded under this account, making it auditable which files
/// came in through the legacy path.
async fn legacy_service_user(s: &Arc<AppState>) -> ApiResult<User> {
    const ID: &str = LEGACY_SERVICE_USER_ID;
    if let Some(u) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(ID)
        .fetch_optional(&s.db)
        .await?
    {
        return Ok(u);
    }
    let role = env_opt("LEGACY_SERVICE_ROLE").unwrap_or_else(|| "editor".into());
    let system_id = env_opt("LEGACY_DEFAULT_SYSTEM").or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"));
    let org_id = env_opt("LEGACY_DEFAULT_ORG");
    sqlx::query(
        r#"INSERT INTO users (id, email, display_name, avatar_tone, password_hash, role, status,
                              default_system_id, default_org_id, source)
           VALUES ($1, 'fileservice@legacy.local', 'Legacy FileService', 'slate', '', $2,
                   'active', $3, $4, 'legacy')
           ON CONFLICT (id) DO NOTHING"#,
    )
    .bind(ID)
    .bind(&role)
    .bind(&system_id)
    .bind(&org_id)
    .execute(&s.db)
    .await?;
    Ok(sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(ID)
        .fetch_one(&s.db)
        .await?)
}

/// The user to record the action as — the real person if known, else the legacy service account.
async fn caller(s: &Arc<AppState>, who: Option<User>) -> ApiResult<User> {
    match who {
        Some(u) => Ok(u),
        None => {
            // Disable this fallback with LEGACY_TRUST_CALLER=0 once all callers are migrated.
            if env_opt("LEGACY_TRUST_CALLER").as_deref() == Some("0") {
                return Err(ApiError::Unauthorized);
            }
            legacy_service_user(s).await
        }
    }
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

fn download_link(id: &Uuid) -> String {
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
            _ => continue,
        };
    }
    Ok(out)
}

async fn upload(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    headers: HeaderMap,
    mut mp: Multipart,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    crate::auth::require_role(&user, &["admin", "editor"])?;
    if user.id == LEGACY_SERVICE_USER_ID {
        // All legacy callers share one service account ⇒ rate-limit per source system,
        // otherwise every caller together would be capped at 60 uploads/min/pod
        // (measured: of 150 requests, only 60 got through).
        let key = format!("legacy:{}", system_from_request(&headers).unwrap_or_else(|| "default".into()));
        let per_min = env_opt("LEGACY_UPLOAD_RATE_PER_MIN").and_then(|v| v.parse().ok()).unwrap_or(1200);
        crate::auth::upload_rate_limit_keyed(&key, per_min).await?;
    } else {
        crate::auth::upload_rate_limit(&user.id).await?;
    }

    let mut up = read_legacy_upload(&s, &mut mp).await?;
    let Some(name) = up.name.clone() else {
        if let Some(st) = up.staged.take() { s.storage.discard(st).await; }
        return Err(ApiError::BadRequest("missing file".into()));
    };
    let staged = up.staged.take().ok_or_else(|| ApiError::BadRequest("missing file body".into()))?;

    // Folder: legacy callers send their own folder ID (e.g. parentFolderId).
    // If ignored, the file lands at the root and callers listing by parentFolderId
    // would not find it, so the legacy ID is "reserved" — a folder is created with
    // the same id (ids are text) ⇒ getFiles?parentFolderId=<legacy id> still works.
    let folder_id = match up.parent_folder_id.as_deref() {
        Some(v) => Some(ensure_legacy_folder(&s, v, &user, system_from_request(&headers).as_deref()).await?),
        None => None,
    };

    // Extra tags from data the legacy caller already sends + a marker for the legacy path.
    // (With no tags supplied, persist_upload_for also adds its automatic tags.)
    let mut extra: Vec<String> = vec!["via:legacy-fileservice".into()];
    if let Some(v) = up.file_type.as_deref() {
        extra.push(format!("type:{v}"));
    }
    if let Some(v) = up.group_id.as_deref() {
        extra.push(format!("group:{v}"));
    }

    let fields = UploadFields {
        name: Some(name),
        body: None,
        staged: Some(staged),
        content_type: up.content_type,
        folder_id,
        system_id: system_from_request(&headers),
        org_id: None,
        project: None,
        status: None,
        owner: None,
        tags: None,
    };
    let file = persist_upload_for(&s, Some(user.id.as_str()), fields, Some(&user)).await?;
    // Append the legacy tags to the automatic ones just added, then re-read the row
    // so `tag_name` in the response matches what was stored (some callers keep it).
    append_tags(&s, &file, &extra).await?;
    let file = find_file(&s, &file.id.to_string()).await?.unwrap_or(file);

    Ok(Json(json!({
        "success": true,
        "status": 200,
        "message": "Upload file success.",
        "data": legacy_file(&file),
    })))
}

/// Reserve a legacy folder ID in FileHub's database (reusing the same id) if it does not exist yet.
async fn ensure_legacy_folder(
    s: &Arc<AppState>,
    legacy_id: &str,
    user: &User,
    want_system: Option<&str>,
) -> ApiResult<String> {
    if let Some(id) = sqlx::query_scalar::<_, String>("SELECT id FROM folders WHERE id = $1")
        .bind(legacy_id)
        .fetch_optional(&s.db)
        .await?
    {
        return Ok(id);
    }
    let system_id: Option<String> =
        sqlx::query_scalar("SELECT default_system_id FROM users WHERE id = $1")
            .bind(&user.id)
            .fetch_optional(&s.db)
            .await?
            .flatten();
    let system_id = want_system
        .map(|v| v.to_string())
        .or(system_id)
        .or_else(|| env_opt("LEGACY_DEFAULT_SYSTEM"))
        .or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"))
        .ok_or_else(|| ApiError::BadRequest("missing system for folder".into()))?;
    sqlx::query(
        r#"INSERT INTO folders (id, system_id, name, owner, created_by)
           VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING"#,
    )
    .bind(legacy_id)
    .bind(&system_id)
    .bind(format!("Legacy folder {legacy_id}"))
    .bind(&user.display_name)
    .bind(&user.id)
    .execute(&s.db)
    .await?;
    Ok(legacy_id.to_string())
}

async fn append_tags(s: &Arc<AppState>, f: &File, extra: &[String]) -> ApiResult<()> {
    let mut tags: Vec<String> = serde_json::from_str(&f.tags).unwrap_or_default();
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
    let Ok(id) = Uuid::parse_str(file_id) else { return Ok(None) };
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
    if crate::auth::ensure_system_access(&s.db, user, &f.system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    // Stream + Range — the legacy service read whole files into memory and ignored
    // Range (asking for 1 MB returned the whole file), so video seeking broke and
    // large files crashed the pod.
    crate::serve::stream_object(s, crate::serve::ServeOpts {
        key: &f.object_key,
        encrypted: f.encrypted,
        name: &f.name,
        etag: f.etag.as_deref(),
        disposition: if inline { "inline" } else { "attachment" },
        cache_control: "private, no-store",
    }, req).await
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
            let user = caller(&s, who).await?;
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
            let user = caller(&s, who).await?;
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
            let user = caller(&s, who).await?;
            if crate::auth::ensure_system_access(&s.db, &user, &f.system_id).await.is_err() {
                return Err(ApiError::NotFound);
            }
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
    Query(q): Query<GetFilesQuery>,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    let limit = q.limit_ofset.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);

    let mut sql = String::from(" WHERE deleted_at IS NULL");
    let mut binds: Vec<String> = Vec::new();
    let mut n = 0usize;
    if let Some(k) = q.keyword.filter(|v| !v.trim().is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND name ILIKE ${n}"));
        binds.push(format!("%{k}%"));
    }
    if let Some(t) = q.tag.filter(|v| !v.trim().is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND tags ILIKE ${n}"));
        binds.push(format!("%{t}%"));
    }
    if let Some(p) = q.parent_folder_id.filter(|v| !v.trim().is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND folder_id = ${n}"));
        binds.push(p);
    }
    // Only systems the caller has access to are visible — same rule as /api/files.
    let scope = crate::auth::effective_system_ids(&s.db, &user).await?;
    let mut scope_bind: Option<Vec<String>> = None;
    if let Some(ids) = scope {
        n += 1;
        sql.push_str(&format!(" AND system_id = ANY(${n})"));
        scope_bind = Some(ids);
    }

    let count_sql = format!("SELECT COUNT(*) FROM files{sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    if let Some(ref ids) = scope_bind {
        cq = cq.bind(ids);
    }
    let total: i64 = cq.fetch_one(&s.db).await?;

    let page_sql = format!(
        "SELECT {FILE_COLS} FROM files{sql} ORDER BY modified_at DESC, id DESC LIMIT ${} OFFSET ${}",
        n + 1,
        n + 2
    );
    let mut pq = sqlx::query_as::<_, File>(&page_sql);
    for b in &binds {
        pq = pq.bind(b);
    }
    if let Some(ref ids) = scope_bind {
        pq = pq.bind(ids);
    }
    let files = pq.bind(limit).bind(offset).fetch_all(&s.db).await?;

    Ok(Json(json!({
        "success": true,
        "message": "get file Successfully.",
        "total": total,
        "data": files.iter().map(legacy_file).collect::<Vec<_>>(),
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
    Json(b): Json<RenameBody>,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    crate::auth::require_role(&user, &["admin", "editor"])?;
    let f = find_file(&s, &b.file_id).await?.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user, &f.system_id).await?;
    if b.rename_to.trim().is_empty() {
        return Err(ApiError::BadRequest("renameTo is required".into()));
    }
    sqlx::query("UPDATE files SET name = $1, modified_at = now() WHERE id = $2")
        .bind(b.rename_to.trim())
        .bind(f.id)
        .execute(&s.db)
        .await?;
    Ok(Json(json!({
        "success": true,
        "status": 200,
        "message": "rename successfully",
    })))
}

#[derive(Deserialize)]
struct TrashBody {
    #[serde(rename = "fileId")]
    file_id: String,
}

async fn move_to_trash(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Json(b): Json<TrashBody>,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    crate::auth::require_role(&user, &["admin", "editor"])?;
    let f = find_file(&s, &b.file_id).await?.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user, &f.system_id).await?;
    sqlx::query("UPDATE files SET deleted_at = now() WHERE id = $1")
        .bind(f.id)
        .execute(&s.db)
        .await?;
    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id,
                                 system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate','deleted',$3,$4,$5,$6,$7,now(),$8)"#,
    )
    .bind(Uuid::now_v7())
    .bind(&user.display_name)
    .bind(&f.name)
    .bind(&f.file_type)
    .bind(f.id)
    .bind(&f.system_id)
    .bind(&f.org_id)
    .bind(&user.id)
    .execute(&s.db)
    .await?;
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
        let id = Uuid::nil();
        assert_eq!(
            download_link(&id),
            format!("/FileService/downloadFile?fileId={id}")
        );
    }

    #[test]
    fn enabled_only_on_explicit_opt_in() {
        std::env::remove_var("LEGACY_FILESERVICE");
        assert!(!enabled());
        std::env::set_var("LEGACY_FILESERVICE", "1");
        assert!(enabled());
        std::env::set_var("LEGACY_FILESERVICE", "0");
        assert!(!enabled());
        std::env::remove_var("LEGACY_FILESERVICE");
    }
}

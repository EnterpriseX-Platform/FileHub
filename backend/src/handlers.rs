use std::sync::Arc;

use axum::{
    extract::{Multipart, Path, Query, State},
    response::IntoResponse,
    Json,
};
use axum::http::{header, HeaderMap, StatusCode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bytes::BytesMut;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use rand::RngCore;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::models::*;
use crate::state::AppState;

/// Column list shared by every `files` read.  Keeps `search_tsv` (added in
/// migration 0004) out so sqlx::FromRow has nothing to map it to.
pub(crate) const FILE_COLS: &str = "id, name, file_type, size_bytes, system_id, org_id, bucket, object_key, project, status, owner, tags, version, metadata, etag, created_at, modified_at, folder_id, encrypted, deleted_at, created_by";

pub async fn health() -> &'static str { "ok" }

/// Deep readiness probe — verifies the DB pool can issue a query and the
/// storage backend can list its root.  Use this on Kubernetes
/// `readinessProbe` so a pod with a broken DB connection drops out of the
/// service mesh.  `/api/health` stays cheap for liveness.
pub async fn ready(State(s): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    // 1. Database round-trip. Cast to bigint — a bare `SELECT 1` is INT4 in
    //    Postgres and decoding it as i64 trips a type mismatch (this probe is
    //    the k8s readinessProbe target, so a 500 here means pods never go
    //    Ready and the rollout hangs).
    let _: i64 = sqlx::query_scalar("SELECT 1::bigint").fetch_one(&s.db).await?;

    // 2. Storage round-trip — write + read + delete a tiny ephemeral blob.
    //    Keeps the probe at O(1) bytes; if the backend is encrypted, the
    //    cipher path is exercised too.
    let probe_key = format!(".healthz/probe-{}", Uuid::now_v7());
    let body = bytes::Bytes::from_static(b"ready");
    let _etag = s.storage.put(&probe_key, body, Some("application/octet-stream")).await?;
    let _ = s.storage.get(&probe_key, s.storage.encryption_enabled()).await?;
    let _ = s.storage.delete(&probe_key).await;

    Ok(Json(serde_json::json!({
        "ready": true,
        "storage": s.storage.backend_label(),
        "encryption": s.storage.encryption_enabled(),
    })))
}

pub async fn stats(State(s): State<Arc<AppState>>) -> ApiResult<Json<DashboardStats>> {
    let (total_files, total_size_bytes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE deleted_at IS NULL"
    ).fetch_one(&s.db).await?;

    let active_orgs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM orgs WHERE status = 'Active'"
    ).fetch_one(&s.db).await?;

    let total_orgs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM orgs"
    ).fetch_one(&s.db).await?;

    let awaiting_review: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM files WHERE status = 'Review' AND deleted_at IS NULL"
    ).fetch_one(&s.db).await?;

    // Personal drives are excluded from the workspace dashboard — their
    // contents are private to each user and shouldn't bleed into a global
    // "Storage by system" chart.
    let storage_by_system: Vec<SystemStorage> = sqlx::query_as(
        r#"SELECT s.id as system_id, s.name, s.tone,
                  COALESCE(SUM(f.size_bytes) FILTER (WHERE f.deleted_at IS NULL), 0)::bigint as size_bytes,
                  COUNT(f.id) FILTER (WHERE f.deleted_at IS NULL)::bigint as file_count
             FROM systems s
             LEFT JOIN files f ON f.system_id = s.id
            WHERE s.deleted_at IS NULL AND s.system_type = 'shared'
            GROUP BY s.id
            ORDER BY size_bytes DESC"#
    ).fetch_all(&s.db).await?;

    let connected_systems: Vec<ConnectedSystem> = sqlx::query_as(
        r#"SELECT s.id, s.name, s.tone, s.status,
                  COUNT(f.id) FILTER (WHERE f.deleted_at IS NULL)::bigint as file_count
             FROM systems s
             LEFT JOIN files f ON f.system_id = s.id
            WHERE s.deleted_at IS NULL AND s.system_type = 'shared'
            GROUP BY s.id
            ORDER BY s.name"#
    ).fetch_all(&s.db).await?;

    // Workspace identity + storage quota live in the `workspace_config` k/v
    // table so admins can rename or re-quota the tenant without redeploying.
    // Pull all three keys in one shot and pivot to typed fields below.
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM workspace_config \
            WHERE key IN ('storage_quota_bytes', 'workspace_name', 'workspace_display')"
    ).fetch_all(&s.db).await?;
    let mut cfg: std::collections::HashMap<String, String> = rows.into_iter().collect();

    let total_quota_bytes: i64 = cfg.remove("storage_quota_bytes")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let workspace_name    = cfg.remove("workspace_name").unwrap_or_default();
    let workspace_display = cfg.remove("workspace_display").unwrap_or_default();

    Ok(Json(DashboardStats {
        total_files,
        total_size_bytes,
        total_quota_bytes,
        active_orgs,
        total_orgs,
        awaiting_review,
        storage_by_system,
        connected_systems,
        workspace_name,
        workspace_display,
        storage_backend:    s.storage.backend_label(),
        encryption_enabled: s.storage.encryption_enabled(),
    }))
}

/// List shared workspace systems.  Personal drives are intentionally hidden
/// here — they live behind `GET /api/personal-drive` so the sidebar and the
/// upload-destination dropdown only show org-visible systems by default.
/// Pass `?include_personal=true` to surface the caller's own My Drive in the
/// same payload (used by the sidebar's "My Drive" section).
#[derive(serde::Deserialize)]
pub struct SystemsQuery {
    pub include_personal: Option<bool>,
}

pub async fn list_systems(
    State(s): State<Arc<AppState>>,
    user: crate::auth::MaybeAuthUser,
    Query(q): Query<SystemsQuery>,
) -> ApiResult<Json<Vec<System>>> {
    // Anonymous + `include_personal=false` → shared only (matches the pre-auth
    // behaviour the dashboard/file-list pages rely on).
    let include_personal = q.include_personal.unwrap_or(false);
    let rows: Vec<System> = match (include_personal, user.0.as_ref()) {
        (true, Some(u)) => sqlx::query_as(
            r#"SELECT * FROM systems
                WHERE deleted_at IS NULL
                  AND (system_type = 'shared' OR owner_user_id = $1)
                ORDER BY system_type DESC, name"#
        )
        .bind(&u.id)
        .fetch_all(&s.db).await?,
        _ => sqlx::query_as(
            r#"SELECT * FROM systems
                WHERE deleted_at IS NULL AND system_type = 'shared'
                ORDER BY name"#
        ).fetch_all(&s.db).await?,
    };
    Ok(Json(rows))
}

/// POST /api/systems — admin-only.  Creates a fresh shared system + bucket
/// (auto-generates a bucket name from the workspace slug if omitted).  We
/// don't pre-create anything on disk: the FsStore/S3Store lazily writes the
/// first object when a file lands.
pub async fn create_system(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<CreateSystem>,
) -> ApiResult<Json<System>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let id     = format!("sys_{}", cuid2::create_id());
    let bucket = body.bucket
        .as_deref()
        .map(|b| b.trim().to_lowercase())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| slugify_bucket(&body.name));

    // Block bucket-name collisions early so we don't leak partial state if
    // the unique index fires.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM systems WHERE bucket = $1)")
        .bind(&bucket).fetch_one(&s.db).await?;
    if exists {
        return Err(ApiError::BadRequest(format!("bucket '{bucket}' already exists")));
    }

    let tone        = body.tone.unwrap_or_else(|| "slate".into());
    let description = body.description;
    let quota       = body.quota_bytes.unwrap_or(0).max(0);

    let row: System = sqlx::query_as(
        r#"INSERT INTO systems
              (id, name, tone, bucket, status, description, system_type, quota_bytes)
           VALUES ($1, $2, $3, $4, 'live', $5, 'shared', $6)
           RETURNING *"#
    )
    .bind(&id).bind(&body.name).bind(&tone).bind(&bucket).bind(description).bind(quota)
    .fetch_one(&s.db).await?;

    // Audit row so admins can trace bucket creation through the activity feed.
    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, system_id, created_at, actor_id)
           VALUES ($1,$2,$3,'created bucket',$4,'system',$5,now(),$6)"#
    )
    .bind(Uuid::now_v7())
    .bind(&user.0.display_name).bind(&user.0.avatar_tone)
    .bind(&row.name).bind(&row.id).bind(&user.0.id)
    .execute(&s.db).await?;
    Ok(Json(row))
}

pub async fn patch_system(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchSystem>,
) -> ApiResult<Json<System>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let cur: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;

    // Personal drives are administered by their owner, not by raw admin
    // PATCH — block accidental renames of My Drive entries here.
    if cur.system_type == "personal" {
        return Err(ApiError::BadRequest("personal drives cannot be modified via PATCH".into()));
    }

    let new_name        = body.name.unwrap_or(cur.name);
    let new_tone        = body.tone.unwrap_or(cur.tone);
    let new_status      = body.status.unwrap_or(cur.status);
    let new_description = body.description.or(cur.description);
    let new_quota       = body.quota_bytes.unwrap_or(cur.quota_bytes).max(0);

    let row: System = sqlx::query_as(
        r#"UPDATE systems
              SET name=$2, tone=$3, status=$4, description=$5, quota_bytes=$6
            WHERE id=$1
            RETURNING *"#
    )
    .bind(&id).bind(&new_name).bind(&new_tone).bind(&new_status).bind(new_description).bind(new_quota)
    .fetch_one(&s.db).await?;

    Ok(Json(row))
}

/// Soft-delete a system.  Refuses if there are any non-trashed files left
/// inside — caller has to clean those up first to avoid orphaning bytes.
pub async fn delete_system(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let cur: Option<System> = sqlx::query_as("SELECT * FROM systems WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&s.db).await?;
    let Some(cur) = cur else { return Err(ApiError::NotFound); };
    if cur.system_type == "personal" {
        return Err(ApiError::BadRequest("personal drives are deleted with their owner".into()));
    }
    let alive: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM files WHERE system_id = $1 AND deleted_at IS NULL"
    ).bind(&id).fetch_one(&s.db).await?;
    if alive > 0 {
        return Err(ApiError::BadRequest(format!("system still has {alive} live file(s); move them first")));
    }
    sqlx::query("UPDATE systems SET deleted_at = now(), status = 'archived' WHERE id = $1")
        .bind(&id).execute(&s.db).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// GET /api/personal-drive — returns the caller's personal drive, creating
/// one on first access if the migration didn't backfill it (e.g. for users
/// added after the migration ran).
pub async fn personal_drive(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<System>> {
    let existing: Option<System> = sqlx::query_as(
        "SELECT * FROM systems WHERE owner_user_id = $1 AND system_type = 'personal' AND deleted_at IS NULL"
    ).bind(&user.0.id).fetch_optional(&s.db).await?;
    if let Some(row) = existing {
        return Ok(Json(row));
    }
    let id     = format!("sys_personal_{}", user.0.id);
    let bucket = format!("personal-{}", user.0.id);
    let row: System = sqlx::query_as(
        r#"INSERT INTO systems
              (id, name, tone, bucket, status, description, system_type, owner_user_id, quota_bytes)
           VALUES ($1, $2, $3, $4, 'live', $5, 'personal', $6, 0)
           ON CONFLICT (id) DO UPDATE SET deleted_at = NULL
           RETURNING *"#
    )
    .bind(&id)
    .bind(format!("My Drive ({})", user.0.display_name))
    .bind(&user.0.avatar_tone)
    .bind(&bucket)
    .bind(format!("Personal drive for {}", user.0.email))
    .bind(&user.0.id)
    .fetch_one(&s.db).await?;
    Ok(Json(row))
}

/// Lower-case, hyphen-only slug for a freshly created system's bucket.  Falls
/// back to `bucket-<6 chars>` if the input degenerates to empty.
fn slugify_bucket(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let trimmed: String = cleaned.trim_matches('-').to_string();
    let collapsed = trimmed
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        format!("bucket-{}", &cuid2::create_id()[..6])
    } else {
        collapsed
    }
}

pub async fn list_orgs(
    State(s): State<Arc<AppState>>,
    Query(q): Query<OrgsQuery>,
) -> ApiResult<Json<Vec<Org>>> {
    let limit  = q.limit.unwrap_or(200).min(2000).max(1);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows: Vec<Org> = if let Some(sid) = q.system_id {
        sqlx::query_as("SELECT * FROM orgs WHERE system_id = $1 ORDER BY name LIMIT $2 OFFSET $3")
            .bind(sid).bind(limit).bind(offset).fetch_all(&s.db).await?
    } else {
        sqlx::query_as("SELECT * FROM orgs ORDER BY name LIMIT $1 OFFSET $2")
            .bind(limit).bind(offset).fetch_all(&s.db).await?
    };
    Ok(Json(rows))
}

pub async fn list_files(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<FilesQuery>,
) -> ApiResult<(axum::http::HeaderMap, Json<Vec<File>>)> {
    let limit  = q.limit.unwrap_or(200).min(2000).max(1);
    let offset = q.offset.unwrap_or(0).max(0);

    // If the caller asked for a specific system, verify they can see it.
    // Without this, a viewer can `?system_id=sys_personal_<admin>` and read
    // every file someone else owns.
    if let Some(ref sid) = q.system_id {
        crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
    }
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;

    // Shared WHERE, built once and bound to both the COUNT and the page query.
    let mut where_sql = String::from(" WHERE deleted_at IS NULL");
    let mut str_binds: Vec<String> = Vec::new();
    let mut vec_binds: Vec<Vec<String>> = Vec::new();
    let mut n = 0usize;

    if let Some(v) = q.system_id { n += 1; where_sql.push_str(&format!(" AND system_id = ${n}")); str_binds.push(v); }
    if let Some(v) = q.org_id    { n += 1; where_sql.push_str(&format!(" AND org_id = ${n}"));    str_binds.push(v); }
    if let Some(v) = q.status    { n += 1; where_sql.push_str(&format!(" AND status = ${n}"));    str_binds.push(v); }
    if let Some(v) = q.project   { n += 1; where_sql.push_str(&format!(" AND project = ${n}"));   str_binds.push(v); }
    if let Some(v) = q.owner     { n += 1; where_sql.push_str(&format!(" AND owner = ${n}"));     str_binds.push(v); }
    // Folder scoping mirrors `list_folders` parent_id: "null"/"" = root (no
    // folder), an id = that folder's files. Absent = every folder (no filter).
    if let Some(v) = q.folder_id {
        if v == "null" || v.is_empty() {
            where_sql.push_str(" AND folder_id IS NULL");
        } else {
            n += 1;
            where_sql.push_str(&format!(" AND folder_id = ${n}"));
            str_binds.push(v);
        }
    }
    // Non-admins are limited to systems they belong to (shared + own personal).
    if let Some(scope_ids) = scope.clone() {
        n += 1;
        where_sql.push_str(&format!(" AND system_id = ANY(${n})"));
        vec_binds.push(scope_ids);
    }

    // Total matching rows (for pagination) — same filters, no limit/offset.
    let count_sql = format!("SELECT COUNT(*) FROM files{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &str_binds { cq = cq.bind(b); }
    for b in &vec_binds { cq = cq.bind(b); }
    let total: i64 = cq.fetch_one(&s.db).await?;

    // Sort: whitelist the column so the param can't inject SQL. Default keeps
    // the previous behaviour (modified_at DESC).
    let sort_col = match q.sort.as_deref() {
        Some("name")    => "name",
        Some("size")    => "size_bytes",
        Some("status")  => "status",
        Some("owner")   => "owner",
        Some("created") => "created_at",
        _               => "modified_at",
    };
    let dir = if q.dir.as_deref() == Some("asc") { "ASC" } else { "DESC" };

    let mut sql = format!("SELECT {FILE_COLS} FROM files{where_sql} ORDER BY {sort_col} {dir}, id {dir}");
    n += 1; let limit_n  = n; sql.push_str(&format!(" LIMIT ${limit_n}"));
    n += 1; let offset_n = n; sql.push_str(&format!(" OFFSET ${offset_n}"));

    let mut query = sqlx::query_as::<_, File>(&sql);
    for b in &str_binds { query = query.bind(b); }
    for b in &vec_binds { query = query.bind(b); }
    query = query.bind(limit).bind(offset);
    let files = query.fetch_all(&s.db).await?;

    // Expose the grand total so clients can paginate (body stays a plain array
    // for backward compatibility — see CORS expose_headers for the browser).
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::HeaderName::from_static("x-total-count"),
        axum::http::HeaderValue::from_str(&total.to_string()).unwrap(),
    );
    Ok((headers, Json(files)))
}

pub async fn file_detail(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<File>> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    Ok(Json(file))
}

/// File-type detector covering the explicit list in TOR 4.15.6.
/// Sanitize a user-supplied filename for safe use in object storage keys.
///
/// Storage backends (filesystem and S3) put the filename directly into a path
/// segment, so any user-controlled bytes there are an injection surface:
/// `/` and `\` traverse, leading `.` makes hidden files, quotes break
/// `Content-Disposition`, and control chars are just nasty.  We replace all
/// of those with `_`, collapse repeated underscores, trim to 200 chars to
/// stay under filesystem name limits, and fall back to a default if the
/// result is empty.  The original name is still stored in the DB so the
/// caller's display name is preserved exactly.
pub fn sanitize_filename(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut leading = true;
    for ch in input.chars() {
        let bad = (ch as u32) < 0x20
            || ch == '/'
            || ch == '\\'
            || ch == '"'
            || (leading && ch == '.');
        if bad {
            out.push('_');
        } else {
            out.push(ch);
            leading = false;
        }
    }
    // Collapse runs of `_` to a single underscore.
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_us = false;
    for ch in out.chars() {
        if ch == '_' {
            if !prev_us { collapsed.push(ch); }
            prev_us = true;
        } else {
            collapsed.push(ch);
            prev_us = false;
        }
    }
    // Trim by character count to ~200 chars so we don't split a UTF-8 codepoint.
    let trimmed: String = collapsed.chars().take(200).collect();
    if trimmed.is_empty() { "upload".to_string() } else { trimmed }
}

pub fn detect_file_type(name: &str) -> String {
    let ext = std::path::Path::new(name)
        .extension().and_then(|e| e.to_str()).unwrap_or("bin").to_lowercase();
    match ext.as_str() {
        "pdf" => "pdf",
        "doc" | "docx" => "docx",
        "xls" | "xlsx" => "xlsx",
        "ppt" | "pptx" => "pptx",
        "html" | "htm" => "html",
        "xml" => "xml",
        "csv" => "csv",
        "json" => "json",
        "zip" => "zip",
        "txt" => "txt",
        "md" => "md",
        "png" => "png",
        "jpg" | "jpeg" => "img",
        "gif" => "img",
        "wav" => "wav",
        "mp3" => "mp3",
        "mp4" | "mov" | "webm" => "mp4",
        _ => "file",
    }.to_string()
}

pub fn is_image_type(t: &str) -> bool {
    matches!(t, "img" | "png" | "jpg" | "jpeg" | "gif")
}

#[derive(Default)]
struct UploadFields {
    name: Option<String>,
    system_id: Option<String>,
    org_id: Option<String>,
    folder_id: Option<String>,
    project: Option<String>,
    status: Option<String>,
    owner: Option<String>,
    tags: Option<String>,
    content_type: Option<String>,
    body: Option<bytes::Bytes>,
}

fn parse_uuid(s: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(s).map_err(|_| ApiError::BadRequest(format!("invalid uuid: {s}")))
}

async fn read_one_upload(multipart: &mut Multipart) -> Result<UploadFields, ApiError> {
    let mut f = UploadFields::default();
    while let Some(mut field) = multipart.next_field().await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                f.name = field.file_name().map(str::to_string);
                f.content_type = field.content_type().map(str::to_string);
                let mut buf = BytesMut::new();
                while let Some(chunk) = field.chunk().await.map_err(|e| ApiError::BadRequest(e.to_string()))? {
                    buf.extend_from_slice(&chunk);
                }
                f.body = Some(buf.freeze());
            }
            "system_id" => f.system_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "org_id"    => f.org_id    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "folder_id" => f.folder_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "project"   => f.project   = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "status"    => f.status    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "owner"     => f.owner     = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "tags"      => f.tags      = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            _ => {}
        }
    }
    Ok(f)
}

/// Nested quota enforcement — workspace → system → org → user.  Every level
/// with a non-zero cap has to fit `used + incoming_bytes`, and the first one
/// that overflows wins.  A zero cap at any level means "no limit at this
/// scope; defer to the next level".  Anonymous uploads skip the user check.
pub(crate) async fn enforce_quota(
    s: &AppState,
    incoming_bytes: i64,
    system_id: Option<&str>,
    org_id: Option<&str>,
    user_id: Option<&str>,
) -> Result<(), ApiError> {
    // ---- Workspace ----
    let ws_cap: Option<i64> = sqlx::query_scalar::<_, String>(
        "SELECT value FROM workspace_config WHERE key = 'storage_quota_bytes'"
    ).fetch_optional(&s.db).await?
        .and_then(|v| v.parse().ok());
    if let Some(cap) = ws_cap.filter(|c| *c > 0) {
        let used: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE deleted_at IS NULL"
        ).fetch_one(&s.db).await?;
        if used + incoming_bytes > cap {
            return Err(ApiError::PayloadTooLarge(format!(
                "workspace quota exceeded: {used}/{cap} bytes used, +{incoming_bytes} requested"
            )));
        }
    }
    // ---- System ----
    if let Some(sid) = system_id {
        let cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM systems WHERE id = $1")
            .bind(sid).fetch_optional(&s.db).await?.unwrap_or(0);
        if cap > 0 {
            let used: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files \
                  WHERE system_id = $1 AND deleted_at IS NULL"
            ).bind(sid).fetch_one(&s.db).await?;
            if used + incoming_bytes > cap {
                return Err(ApiError::PayloadTooLarge(format!(
                    "system '{sid}' quota exceeded: {used}/{cap} bytes used"
                )));
            }
        }
    }
    // ---- Org ----
    if let Some(oid) = org_id {
        let cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM orgs WHERE id = $1")
            .bind(oid).fetch_optional(&s.db).await?.unwrap_or(0);
        if cap > 0 {
            let used: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files \
                  WHERE org_id = $1 AND deleted_at IS NULL"
            ).bind(oid).fetch_one(&s.db).await?;
            if used + incoming_bytes > cap {
                return Err(ApiError::PayloadTooLarge(format!(
                    "org '{oid}' quota exceeded: {used}/{cap} bytes used"
                )));
            }
        }
    }
    // ---- User ----
    if let Some(uid) = user_id {
        let cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM users WHERE id = $1")
            .bind(uid).fetch_optional(&s.db).await?.unwrap_or(0);
        if cap > 0 {
            let used: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files \
                  WHERE created_by = $1 AND deleted_at IS NULL"
            ).bind(uid).fetch_one(&s.db).await?;
            if used + incoming_bytes > cap {
                return Err(ApiError::PayloadTooLarge(format!(
                    "user '{uid}' quota exceeded: {used}/{cap} bytes used"
                )));
            }
        }
    }
    Ok(())
}

async fn persist_upload(s: &AppState, actor: Option<&str>, f: UploadFields) -> ApiResult<File> {
    let name = f.name.ok_or_else(|| ApiError::BadRequest("missing file".into()))?;
    let body = f.body.ok_or_else(|| ApiError::BadRequest("missing file body".into()))?;
    let system_id = f.system_id.ok_or_else(|| ApiError::BadRequest("missing system_id".into()))?;

    let system: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1")
        .bind(&system_id).fetch_optional(&s.db).await?
        .ok_or_else(|| ApiError::BadRequest("unknown system_id".into()))?;

    let size_bytes = body.len() as i64;
    enforce_quota(s, size_bytes, Some(&system_id), f.org_id.as_deref(), actor).await?;

    let id = Uuid::now_v7();
    let file_type = detect_file_type(&name);
    // Keep `name` in the DB so the user-facing display is preserved verbatim,
    // but rewrite the on-disk key through sanitize_filename so a malicious
    // filename can't traverse paths, inject quotes, or leave control bytes
    // in storage keys.
    let safe = sanitize_filename(&name);
    let object_key = format!("{}/{}-{}", system.bucket, id, safe);
    let ct = f.content_type.clone()
        .or_else(|| mime_guess::from_path(&name).first().map(|m| m.to_string()));

    let etag = s.storage.put(&object_key, body, ct.as_deref()).await?;
    let encrypted = s.storage.encryption_enabled();

    let owner = f.owner.unwrap_or_else(|| "Anonymous".into());
    let status = f.status.unwrap_or_else(|| "Draft".into());
    let tags_json = f.tags.unwrap_or_else(|| "[]".into());
    let now = Utc::now();

    sqlx::query(
        r#"INSERT INTO files
            (id, name, file_type, size_bytes, system_id, org_id, folder_id, bucket, object_key,
             project, status, owner, tags, version, metadata, etag, encrypted, created_at, modified_at, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,1,'{}',$14,$15,$16,$16,$17)"#
    )
    .bind(id).bind(&name).bind(&file_type).bind(size_bytes)
    .bind(&system_id).bind(&f.org_id).bind(&f.folder_id)
    .bind(&system.bucket).bind(&object_key)
    .bind(&f.project).bind(&status).bind(&owner).bind(&tags_json)
    .bind(&etag).bind(encrypted).bind(now).bind(actor)
    .execute(&s.db).await?;

    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"#
    )
    .bind(Uuid::now_v7())
    .bind(&owner).bind(&system.tone).bind("uploaded")
    .bind(&name).bind(&file_type).bind(id)
    .bind(&system_id).bind(&f.org_id).bind(now).bind(actor)
    .execute(&s.db).await?;

    // P1: extract text + generate a thumbnail + render Office → PDF preview.
    // We re-fetch the plaintext body from storage so the encryption layer
    // hands us decrypted bytes; we already wrote it once and don't want to
    // keep a second copy in memory.
    let mut ocr_scheduled = false;
    if let Some((bytes, _)) = s.storage.get(&object_key, encrypted).await.ok().flatten() {
        let extracted = crate::p1::extract_text_from(&file_type, &bytes);
        if let Some(text) = &extracted {
            crate::p1::index_file_content(&s.db, id, text).await;
        }
        crate::p1::index_thumbnail(&s.db, id, &file_type, &bytes).await;
        crate::p1::index_office_preview(&s.db, id, &file_type, &name, &bytes).await;

        // OCR fallback for scanned images / image-only PDFs (no extractable
        // text). Runs in the background so the upload response stays fast; when
        // it recognises text it indexes it for full-text search and enqueues AI.
        if extracted.is_none() && crate::ocr::enabled() && crate::ocr::is_ocrable(&file_type) {
            ocr_scheduled = true;
            let db = s.db.clone();
            let ft = file_type.clone();
            let ai_on = s.ai.enabled();
            tokio::spawn(async move {
                if let Some(text) = crate::ocr::ocr_extract(&ft, &bytes).await {
                    crate::p1::index_file_content(&db, id, &text).await;
                    if ai_on {
                        crate::ai_worker::enqueue(&db, id).await;
                    }
                }
            });
        }
    }

    // AI-native: enqueue understanding (embed + summarise). Async + best-effort,
    // so the upload response stays fast and an AI outage never blocks uploads.
    // When OCR is scheduled, that task enqueues AI once the text is ready.
    if s.ai.enabled() && !ocr_scheduled {
        crate::ai_worker::enqueue(&s.db, id).await;
    }

    Ok(sqlx::query_as::<_, File>(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1"))
        .bind(id).fetch_one(&s.db).await?)
}

pub async fn upload_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    mut multipart: Multipart,
) -> ApiResult<Json<File>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    crate::auth::upload_rate_limit(&user.0.id).await?;
    let fields = read_one_upload(&mut multipart).await?;
    if let Some(ref sid) = fields.system_id {
        crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
    }
    Ok(Json(persist_upload(&s, Some(user.0.id.as_str()), fields).await?))
}

/// Batch upload — TOR 4.15.9.
pub async fn upload_batch(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    mut multipart: Multipart,
) -> ApiResult<Json<Vec<File>>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let mut shared = UploadFields::default();
    let mut out: Vec<File> = Vec::new();

    while let Some(mut field) = multipart.next_field().await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let mut one = UploadFields {
                    system_id: shared.system_id.clone(),
                    org_id:    shared.org_id.clone(),
                    folder_id: shared.folder_id.clone(),
                    project:   shared.project.clone(),
                    status:    shared.status.clone(),
                    owner:     shared.owner.clone(),
                    tags:      shared.tags.clone(),
                    ..Default::default()
                };
                one.name = field.file_name().map(str::to_string);
                one.content_type = field.content_type().map(str::to_string);
                let mut buf = BytesMut::new();
                while let Some(chunk) = field.chunk().await.map_err(|e| ApiError::BadRequest(e.to_string()))? {
                    buf.extend_from_slice(&chunk);
                }
                one.body = Some(buf.freeze());
                if let Some(ref sid) = one.system_id {
                    crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
                }
                // Charge the rate limiter PER FILE — a batch of N files spends N
                // tokens, matching the per-file cap (charging once per request
                // would let a single token persist unlimited files).
                crate::auth::upload_rate_limit(&user.0.id).await?;
                out.push(persist_upload(&s, Some(user.0.id.as_str()), one).await?);
            }
            "system_id" => shared.system_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "org_id"    => shared.org_id    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "folder_id" => shared.folder_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "project"   => shared.project   = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "status"    => shared.status    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "owner"     => shared.owner     = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "tags"      => shared.tags      = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            _ => {}
        }
    }

    if out.is_empty() {
        return Err(ApiError::BadRequest("no files provided".into()));
    }
    Ok(Json(out))
}

pub async fn download_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    headers_in: HeaderMap,
) -> ApiResult<axum::response::Response> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }

    if let (Some(etag), Some(inm)) = (&file.etag, headers_in.get(header::IF_NONE_MATCH)) {
        if inm.to_str().ok().map(|v| v.trim_matches('"')) == Some(etag.as_str()) {
            let mut h = HeaderMap::new();
            h.insert(header::ETAG, format!("\"{etag}\"").parse().unwrap());
            return Ok((StatusCode::NOT_MODIFIED, h).into_response());
        }
    }

    let (body, ct) = s.storage.get(&file.object_key, file.encrypted).await?
        .ok_or(ApiError::NotFound)?;
    let total: u64 = body.len() as u64;
    let content_type = ct.unwrap_or_else(|| "application/octet-stream".into());

    // Video / audio file types render through `<video>` / `<audio>` elements,
    // which the browser will only treat as streamable if the response sets
    // `Accept-Ranges: bytes` *and* honours `Range:` headers with 206 Partial
    // Content.  Without that, a 100 MB video plays only after the full bytes
    // arrive and the user can't seek.  This block matches the relevant slice
    // of the in-memory `Bytes`, then re-uses the headers below.
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    // Inline disposition for media types so the `<video>`/`<audio>` tag can
    // actually play them; everything else stays as an `attachment` so a click
    // on the topbar Download button triggers a real download.
    let inline = matches!(file.file_type.as_str(),
        "mp4" | "mov" | "webm" | "mp3" | "wav" | "img" | "png" | "jpg" | "jpeg" | "gif" | "pdf");
    // Content-Disposition needs both an ASCII fallback (`filename="..."`) for
    // ancient clients and an RFC 5987 `filename*=UTF-8''<percent-encoded>`
    // form for browsers — without the latter, Thai or any non-ASCII filename
    // ends up as a string of `?`s in the Save As dialog.  The ASCII fallback
    // is run through `sanitize_filename` so a name containing `"` doesn't
    // break the quoted form (and accidentally inject extra header params).
    let kind = if inline { "inline" } else { "attachment" };
    let ascii_fallback = sanitize_filename(&file.name);
    let utf8_encoded   = urlencoding::encode(&file.name);
    let disp = format!(
        "{kind}; filename=\"{ascii_fallback}\"; filename*=UTF-8''{utf8_encoded}"
    );
    h.insert(header::CONTENT_DISPOSITION, disp.parse().unwrap());
    if let Some(etag) = &file.etag {
        h.insert(header::ETAG, format!("\"{etag}\"").parse().unwrap());
    }
    let cache_control = if is_image_type(&file.file_type) {
        "public, max-age=86400, immutable"
    } else {
        "private, no-store"
    };
    h.insert(header::CACHE_CONTROL, cache_control.parse().unwrap());
    h.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());

    // Range request? Honour it.  Format is e.g. `Range: bytes=0-1023` or
    // `Range: bytes=1024-` (open-ended).  We only implement single-range
    // requests — multipart byte ranges are rare and not needed for media
    // playback in any modern browser.
    if let Some(range) = headers_in.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        if let Some((start, end)) = parse_range(range, total) {
            let len = end - start + 1;
            let slice = body.slice((start as usize)..((end as usize) + 1));
            h.insert(header::CONTENT_LENGTH, len.to_string().parse().unwrap());
            h.insert(
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{total}").parse().unwrap(),
            );
            return Ok((StatusCode::PARTIAL_CONTENT, h, slice).into_response());
        }
        // Malformed range header → 416 with a hint of the real size so the
        // client can retry with a valid window.
        h.insert(
            header::CONTENT_RANGE,
            format!("bytes */{total}").parse().unwrap(),
        );
        return Ok((StatusCode::RANGE_NOT_SATISFIABLE, h).into_response());
    }

    h.insert(header::CONTENT_LENGTH, total.to_string().parse().unwrap());
    Ok((StatusCode::OK, h, body).into_response())
}

/// Parse a single-range `bytes=start-end` header into an inclusive `(start, end)`
/// pair, clamped to the file size.  Returns `None` for multipart, unit
/// other than `bytes`, or an end before the start.
fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    let rest = header.strip_prefix("bytes=")?;
    if rest.contains(',') { return None; }              // multipart unsupported
    let (s, e) = rest.split_once('-')?;
    let start: u64 = if s.is_empty() {
        // Suffix range like `bytes=-1024` ⇒ last 1024 bytes.
        let suffix: u64 = e.parse().ok()?;
        total.saturating_sub(suffix)
    } else {
        s.parse().ok()?
    };
    let end: u64 = if e.is_empty() {
        total.saturating_sub(1)
    } else {
        e.parse().ok()?
    };
    if start > end || start >= total { return None; }
    Some((start, end.min(total - 1)))
}

/// Soft delete — moves a file to the trash by setting `deleted_at`.  Use
/// `?hard=true` (admin role required) to permanently purge.
#[derive(Debug, serde::Deserialize)]
pub struct DeleteQuery {
    pub hard: Option<bool>,
}

pub async fn delete_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<StatusCode> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await?;

    if q.hard.unwrap_or(false) {
        crate::auth::require_role(&user.0, &["admin"])?;
        let version_keys: Vec<(String,)> = sqlx::query_as(
            "SELECT object_key FROM file_versions WHERE file_id = $1"
        ).bind(id).fetch_all(&s.db).await?;
        sqlx::query("DELETE FROM files WHERE id = $1").bind(id).execute(&s.db).await?;
        let _ = s.storage.delete(&file.object_key).await;
        for (key,) in version_keys {
            let _ = s.storage.delete(&key).await;
        }
    } else {
        crate::auth::require_role(&user.0, &["admin", "editor"])?;
        sqlx::query("UPDATE files SET deleted_at = now(), modified_at = now() WHERE id = $1")
            .bind(id).execute(&s.db).await?;
        sqlx::query(
            r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
               VALUES ($1,$2,'slate','sent to trash',$3,$4,$5,$6,$7,now(),$8)"#,
        )
        .bind(Uuid::now_v7()).bind(&user.0.display_name)
        .bind(&file.name).bind(&file.file_type).bind(file.id)
        .bind(file.system_id).bind(file.org_id).bind(user.0.id)
        .execute(&s.db).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// List soft-deleted files (the Trash bin).
pub async fn list_trash(State(s): State<Arc<AppState>>) -> ApiResult<Json<Vec<File>>> {
    let rows: Vec<File> = sqlx::query_as(&format!(
        "SELECT {FILE_COLS} FROM files WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC LIMIT 500"
    )).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

pub async fn restore_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<File>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let file: Option<File> = sqlx::query_as(&format!(
        "SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NOT NULL"
    )).bind(id).fetch_optional(&s.db).await?;
    let file = file.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await?;

    sqlx::query("UPDATE files SET deleted_at = NULL, modified_at = now() WHERE id = $1")
        .bind(id).execute(&s.db).await?;
    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate','restored',$3,$4,$5,$6,$7,now(),$8)"#,
    )
    .bind(Uuid::now_v7()).bind(&user.0.display_name)
    .bind(&file.name).bind(&file.file_type).bind(file.id)
    .bind(file.system_id).bind(file.org_id).bind(user.0.id)
    .execute(&s.db).await?;

    Ok(Json(sqlx::query_as::<_, File>(&format!(
        "SELECT {FILE_COLS} FROM files WHERE id = $1"
    )).bind(id).fetch_one(&s.db).await?))
}

pub async fn list_activity(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ActivityQuery>,
) -> ApiResult<Json<Vec<Activity>>> {
    let limit  = q.limit.unwrap_or(50).min(500).max(1);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows: Vec<Activity> = sqlx::query_as("SELECT * FROM activity ORDER BY created_at DESC LIMIT $1 OFFSET $2")
        .bind(limit).bind(offset).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

pub async fn list_views(State(s): State<Arc<AppState>>) -> ApiResult<Json<Vec<View>>> {
    Ok(Json(sqlx::query_as("SELECT * FROM views ORDER BY pinned DESC, created_at DESC")
        .fetch_all(&s.db).await?))
}

pub async fn create_view(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(v): Json<CreateView>,
) -> ApiResult<Json<View>> {
    // Views are a system table → CUID2 PK with `vw_` prefix.
    let id = format!("vw_{}", cuid2::create_id());
    sqlx::query(
        r#"INSERT INTO views (id, name, layout, source, filters, group_by, sort_by, fields, pinned, color, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"#
    )
    .bind(&id)
    .bind(&v.name)
    .bind(v.layout.unwrap_or_else(|| "table".into()))
    .bind("{}")
    .bind(v.filters.map(|f| f.to_string()).unwrap_or_else(|| "[]".into()))
    .bind(&v.group_by)
    .bind(&v.sort_by)
    .bind(v.fields.map(|f| f.to_string()).unwrap_or_else(|| "[]".into()))
    .bind(v.pinned.unwrap_or(false))
    .bind(&v.color)
    .bind(&user.0.id)
    .execute(&s.db).await?;

    Ok(Json(sqlx::query_as::<_, View>("SELECT * FROM views WHERE id = $1")
        .bind(&id).fetch_one(&s.db).await?))
}

pub async fn list_permissions(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Permission>>> {
    // Missing file → empty list (preserve the existing "permissions for
    // unknown id is just empty" semantic that the test suite locks in).
    // Access denial still returns an empty list — same observable behaviour
    // as a missing file so we don't leak existence.
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    if let Some((system_id,)) = row {
        if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
            return Ok(Json(vec![]));
        }
    } else {
        return Ok(Json(vec![]));
    }
    Ok(Json(sqlx::query_as("SELECT * FROM permissions WHERE file_id = $1 ORDER BY created_at")
        .bind(file_id).fetch_all(&s.db).await?))
}

// -----------------------------------------------------------------------------
// Search — TOR 4.15.14
// -----------------------------------------------------------------------------
pub async fn search_files(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<File>>> {
    let limit = q.limit.unwrap_or(50).min(500).max(1);
    let term = q.q.trim();
    if term.is_empty() {
        return Err(ApiError::BadRequest("q must not be empty".into()));
    }

    // If the caller asked for a specific system, verify they can see it.
    if let Some(ref sid) = q.system_id {
        crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
    }
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;

    let like_pattern = format!("%{}%", term);
    let mut sql = format!(
        "SELECT {FILE_COLS} \
         FROM files \
         WHERE deleted_at IS NULL AND ( \
            search_tsv @@ plainto_tsquery('simple', $1) \
            OR name ILIKE $2 \
            OR tags ILIKE $2 \
            OR coalesce(owner,'') ILIKE $2 \
            OR coalesce(project,'') ILIKE $2 \
            OR id IN (SELECT file_id FROM file_content WHERE content_tsv @@ plainto_tsquery('simple', $1)) \
         )"
    );
    let mut str_binds: Vec<String> = vec![term.to_string(), like_pattern];
    let mut vec_binds: Vec<Vec<String>> = Vec::new();
    let mut n = 2usize;

    if let Some(v) = q.system_id { n += 1; sql.push_str(&format!(" AND system_id = ${n}")); str_binds.push(v); }
    if let Some(v) = q.file_type { n += 1; sql.push_str(&format!(" AND file_type = ${n}")); str_binds.push(v); }
    if let Some(v) = q.status    { n += 1; sql.push_str(&format!(" AND status = ${n}"));    str_binds.push(v); }
    // Non-admins are limited to systems they belong to (shared + own personal).
    if let Some(scope_ids) = scope {
        n += 1;
        sql.push_str(&format!(" AND system_id = ANY(${n})"));
        vec_binds.push(scope_ids);
    }
    n += 1;
    sql.push_str(&format!(" ORDER BY modified_at DESC LIMIT ${n}"));

    let mut query = sqlx::query_as::<_, File>(&sql);
    for b in &str_binds { query = query.bind(b); }
    for b in &vec_binds { query = query.bind(b); }
    query = query.bind(limit);

    Ok(Json(query.fetch_all(&s.db).await?))
}

// -----------------------------------------------------------------------------
// Versioning — TOR 4.15.7
// -----------------------------------------------------------------------------
pub async fn list_versions(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<FileVersion>>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let rows: Vec<FileVersion> = sqlx::query_as(
        "SELECT * FROM file_versions WHERE file_id = $1 ORDER BY version DESC"
    ).bind(id).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

pub async fn upload_version(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Json<File>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await?;

    // Check-out lock (TOR 5.3.8.4): a file checked out by someone else can't be
    // updated by anyone but the holder.
    if let Some(holder) = crate::checkout::lock_blocks(&s.db, id, &user.0.id).await {
        return Err(ApiError::Conflict(format!("checked out by {holder} — check in first to upload a new version")));
    }

    let mut body: Option<BytesMut> = None;
    let mut content_type: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut uploaded_by: Option<String> = None;
    let mut note: Option<String> = None;

    while let Some(mut field) = multipart.next_field().await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
    {
        match field.name().unwrap_or("") {
            "file" => {
                filename = field.file_name().map(str::to_string);
                content_type = field.content_type().map(str::to_string);
                let mut buf = BytesMut::new();
                while let Some(chunk) = field.chunk().await.map_err(|e| ApiError::BadRequest(e.to_string()))? {
                    buf.extend_from_slice(&chunk);
                }
                body = Some(buf);
            }
            "uploaded_by" => uploaded_by = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "note"        => note        = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            _ => {}
        }
    }

    let body = body.ok_or_else(|| ApiError::BadRequest("missing file body".into()))?.freeze();
    let uploaded_by = uploaded_by.unwrap_or_else(|| user.0.display_name.clone());

    let prev_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO file_versions (id, file_id, version, object_key, size_bytes, etag, uploaded_by, note)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#
    )
    .bind(prev_id).bind(file.id).bind(file.version)
    .bind(&file.object_key).bind(file.size_bytes).bind(&file.etag)
    .bind(&uploaded_by).bind(&note)
    .execute(&s.db).await?;

    let new_version = file.version + 1;
    let display_name = filename.clone().unwrap_or_else(|| file.name.clone());
    let safe = sanitize_filename(&display_name);
    let new_object_key = format!("{}/{}-v{}-{}", file.bucket, file.id, new_version, safe);
    let size_bytes = body.len() as i64;
    enforce_quota(&s, size_bytes,
                  Some(&file.system_id),
                  file.org_id.as_deref(),
                  Some(&user.0.id)).await?;
    let ct = content_type.or_else(|| mime_guess::from_path(&display_name).first().map(|m| m.to_string()));
    let etag = s.storage.put(&new_object_key, body, ct.as_deref()).await?;
    let encrypted = s.storage.encryption_enabled();
    let now = Utc::now();

    sqlx::query(
        r#"UPDATE files
              SET version     = $1,
                  object_key  = $2,
                  size_bytes  = $3,
                  etag        = $4,
                  encrypted   = $5,
                  modified_at = $6
            WHERE id = $7"#
    )
    .bind(new_version).bind(&new_object_key).bind(size_bytes).bind(&etag).bind(encrypted).bind(now).bind(file.id)
    .execute(&s.db).await?;

    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate',$3,$4,$5,$6,$7,$8,$9,$10)"#
    )
    .bind(Uuid::now_v7())
    .bind(&uploaded_by)
    .bind(format!("uploaded v{} of", new_version))
    .bind(&file.name).bind(&file.file_type).bind(file.id)
    .bind(file.system_id).bind(file.org_id).bind(now).bind(user.0.id)
    .execute(&s.db).await?;

    let updated: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1"))
        .bind(file.id).fetch_one(&s.db).await?;
    Ok(Json(updated))
}

// -----------------------------------------------------------------------------
// Move / rename — TOR 4.15.12
// -----------------------------------------------------------------------------
pub async fn patch_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(p): Json<PatchFile>,
) -> ApiResult<Json<File>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await?;

    let new_system: Option<System> = match &p.system_id {
        Some(sid) if sid != &file.system_id => {
            crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
            Some(
                sqlx::query_as("SELECT * FROM systems WHERE id = $1")
                    .bind(sid).fetch_optional(&s.db).await?
                    .ok_or_else(|| ApiError::BadRequest("unknown system_id".into()))?
            )
        },
        _ => None,
    };

    let new_name = p.name.clone().unwrap_or_else(|| file.name.clone());
    let mut new_object_key = file.object_key.clone();
    let mut new_bucket = file.bucket.clone();

    if let Some(sys) = &new_system {
        new_bucket = sys.bucket.clone();
        let safe = sanitize_filename(&new_name);
        new_object_key = format!("{}/{}-{}", sys.bucket, file.id, safe);
        if let Some((body, ct)) = s.storage.get(&file.object_key, file.encrypted).await? {
            s.storage.put(&new_object_key, body, ct.as_deref()).await?;
            let _ = s.storage.delete(&file.object_key).await;
        }
    } else if p.name.is_some() && new_name != file.name {
        let safe = sanitize_filename(&new_name);
        new_object_key = format!("{}/{}-{}", file.bucket, file.id, safe);
        if new_object_key != file.object_key {
            if let Some((body, ct)) = s.storage.get(&file.object_key, file.encrypted).await? {
                s.storage.put(&new_object_key, body, ct.as_deref()).await?;
                let _ = s.storage.delete(&file.object_key).await;
            }
        }
    }

    let tags_json = match p.tags {
        Some(v) => v.to_string(),
        None    => file.tags.clone(),
    };

    let new_file_type = if p.name.is_some() {
        detect_file_type(&new_name)
    } else {
        file.file_type.clone()
    };

    let new_folder_id: Option<String> = match p.folder_id {
        Some(v) => v,
        None    => file.folder_id.clone(),
    };

    let now = Utc::now();
    let new_system_id = p.system_id.clone().unwrap_or_else(|| file.system_id.clone());
    let new_org_id    = p.org_id.clone().or(file.org_id.clone());
    let new_project   = p.project.or(file.project.clone());
    let new_status    = p.status.unwrap_or_else(|| file.status.clone());
    let new_owner     = p.owner.unwrap_or_else(|| file.owner.clone());

    // Folder move is now reachable from the table's bulk "Move to folder" UI.
    // files.folder_id's FK only guarantees the id exists *somewhere*, so a
    // caller could otherwise relocate a file into another system's folder
    // (including another user's personal drive). When the folder actually
    // changes, require the target to live in the file's (new) system — the
    // caller already passed ensure_system_access on that system above.
    if let Some(ref fid) = new_folder_id {
        if file.folder_id.as_deref() != Some(fid.as_str()) {
            let folder_sys: Option<(String,)> = sqlx::query_as(
                "SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL",
            ).bind(fid).fetch_optional(&s.db).await?;
            let (folder_sys,) = folder_sys
                .ok_or_else(|| ApiError::BadRequest("unknown folder_id".into()))?;
            if folder_sys != new_system_id {
                return Err(ApiError::BadRequest("folder_id is not in the file's system".into()));
            }
        }
    }

    sqlx::query(
        r#"UPDATE files
              SET name        = $1,
                  file_type   = $2,
                  system_id   = $3,
                  org_id      = $4,
                  bucket      = $5,
                  object_key  = $6,
                  project     = $7,
                  status      = $8,
                  owner       = $9,
                  tags        = $10,
                  folder_id   = $11,
                  modified_at = $12
            WHERE id = $13"#
    )
    .bind(&new_name).bind(&new_file_type).bind(&new_system_id).bind(&new_org_id)
    .bind(&new_bucket).bind(&new_object_key).bind(&new_project).bind(&new_status)
    .bind(&new_owner).bind(&tags_json).bind(&new_folder_id).bind(now).bind(file.id)
    .execute(&s.db).await?;

    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate',$3,$4,$5,$6,$7,$8,$9,$10)"#
    )
    .bind(Uuid::now_v7())
    .bind(&user.0.display_name)
    .bind(if new_system.is_some() { "moved" } else { "updated" })
    .bind(&new_name).bind(&new_file_type).bind(file.id)
    .bind(&new_system_id).bind(&new_org_id).bind(now).bind(&user.0.id)
    .execute(&s.db).await?;

    let file = sqlx::query_as::<_, File>(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1"))
        .bind(file.id).fetch_one(&s.db).await?;
    Ok(Json(file))
}

// -----------------------------------------------------------------------------
// Folders — TOR 4.15.4
// -----------------------------------------------------------------------------
pub async fn list_folders(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<FoldersQuery>,
) -> ApiResult<Json<Vec<Folder>>> {
    // If the caller asked for a specific system, verify they can see it.
    if let Some(ref sid) = q.system_id {
        crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
    }
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;

    let mut sql = String::from("SELECT * FROM folders WHERE deleted_at IS NULL");
    let mut str_binds: Vec<String> = Vec::new();
    let mut vec_binds: Vec<Vec<String>> = Vec::new();
    let mut n = 0usize;
    if let Some(v) = q.system_id { n += 1; sql.push_str(&format!(" AND system_id = ${n}")); str_binds.push(v); }
    if let Some(v) = q.parent_id {
        if v == "null" || v.is_empty() {
            sql.push_str(" AND parent_id IS NULL");
        } else {
            n += 1;
            sql.push_str(&format!(" AND parent_id = ${n}"));
            str_binds.push(v);
        }
    }
    // Non-admins are limited to systems they belong to (shared + own personal).
    if let Some(scope_ids) = scope {
        n += 1;
        sql.push_str(&format!(" AND system_id = ANY(${n})"));
        vec_binds.push(scope_ids);
    }
    let _ = n;
    sql.push_str(" ORDER BY name");
    let mut q = sqlx::query_as::<_, Folder>(&sql);
    for b in str_binds { q = q.bind(b); }
    for b in vec_binds { q = q.bind(b); }
    Ok(Json(q.fetch_all(&s.db).await?))
}

pub async fn create_folder(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(c): Json<CreateFolder>,
) -> ApiResult<Json<Folder>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    crate::auth::ensure_system_access(&s.db, &user.0, &c.system_id).await?;
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM systems WHERE id = $1")
        .bind(&c.system_id).fetch_optional(&s.db).await?;
    if exists.is_none() {
        return Err(ApiError::BadRequest("unknown system_id".into()));
    }
    if let Some(ref pid) = c.parent_id {
        let p: Option<(String,)> = sqlx::query_as("SELECT id FROM folders WHERE id = $1")
            .bind(pid).fetch_optional(&s.db).await?;
        if p.is_none() {
            return Err(ApiError::BadRequest("unknown parent_id".into()));
        }
    }

    // System tables use CUID2 — readable + sortable enough for system rows.
    let id = format!("fld_{}", cuid2::create_id());
    let owner = c.owner.unwrap_or_else(|| user.0.display_name.clone());
    sqlx::query(
        r#"INSERT INTO folders (id, system_id, org_id, parent_id, name, color, owner, encrypted, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#
    )
    .bind(&id).bind(&c.system_id).bind(&c.org_id).bind(&c.parent_id)
    .bind(&c.name).bind(&c.color).bind(&owner).bind(c.encrypted.unwrap_or(false)).bind(&user.0.id)
    .execute(&s.db).await?;

    Ok(Json(sqlx::query_as::<_, Folder>("SELECT * FROM folders WHERE id = $1")
        .bind(&id).fetch_one(&s.db).await?))
}

pub async fn delete_folder(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL"
    ).bind(&id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;
    let r = sqlx::query("UPDATE folders SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).execute(&s.db).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// -----------------------------------------------------------------------------
// Share links — TOR 4.15.11
// -----------------------------------------------------------------------------
pub fn random_token() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn create_share_link(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
    Json(c): Json<CreateShareLink>,
) -> ApiResult<Json<ShareResponse>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    // Workspace access policy: an admin can switch off external link sharing
    // (Settings → Access policy → allow_external_sharing). Absent key → allowed,
    // so existing deployments are unaffected until they explicitly opt out.
    let ext = sqlx::query_scalar::<_, String>(
        "SELECT value FROM workspace_config WHERE key = 'allow_external_sharing'"
    ).fetch_optional(&s.db).await?;
    if matches!(ext.as_deref(), Some("false") | Some("0") | Some("off")) {
        return Err(ApiError::Forbidden);
    }
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(file_id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await?;

    let token = random_token();
    // Share links are public credentials — a TTL is mandatory. We default to
    // 7 days when the caller omits `expires_in_days` and cap to 90 even when
    // the caller asks for more, so a stale email can't unlock a file forever.
    const DEFAULT_TTL_DAYS: i64 = 7;
    const MAX_TTL_DAYS:     i64 = 90;
    let ttl_days = c.expires_in_days.unwrap_or(DEFAULT_TTL_DAYS).clamp(1, MAX_TTL_DAYS);
    let expires_at = Some(Utc::now() + Duration::days(ttl_days));
    let id = Uuid::now_v7();
    // `created_by` is a display string — we always set it from the verified
    // session.  Allowing the caller to spoof it (former behaviour) let any
    // editor mint share links that look like they came from an admin.
    let created_by = user.0.display_name.clone();
    let _ = c.created_by; // intentionally ignored, kept for backward-compat request shape

    sqlx::query(
        r#"INSERT INTO share_links (id, file_id, token, expires_at, created_by, note, created_by_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7)"#
    )
    .bind(id).bind(file.id).bind(&token).bind(expires_at).bind(&created_by).bind(&c.note).bind(&user.0.id)
    .execute(&s.db).await?;

    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate','shared link for',$3,$4,$5,$6,$7,now(),$8)"#
    )
    .bind(Uuid::now_v7())
    .bind(&created_by).bind(&file.name).bind(&file.file_type)
    .bind(file.id).bind(&file.system_id).bind(&file.org_id).bind(&user.0.id)
    .execute(&s.db).await?;

    Ok(Json(ShareResponse {
        // Returned URL is user-facing — goes through the frontend basePath
        // so it works copy-pasted into an email.
        url: format!("/filehub/api/share/{token}"),
        token,
        expires_at,
        file,
    }))
}

async fn share_target(s: &AppState, token: &str) -> ApiResult<File> {
    let row: Option<(Uuid, Option<chrono::DateTime<Utc>>)> = sqlx::query_as(
        "SELECT file_id, expires_at FROM share_links WHERE token = $1"
    ).bind(token).fetch_optional(&s.db).await?;
    let (file_id, expires_at) = row.ok_or(ApiError::NotFound)?;
    if let Some(exp) = expires_at {
        if Utc::now() > exp {
            return Err(ApiError::NotFound);
        }
    }
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(file_id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    Ok(file)
}

pub async fn share_meta(
    State(s): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> ApiResult<Json<File>> {
    Ok(Json(share_target(&s, &token).await?))
}

pub async fn share_download(
    State(s): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> ApiResult<axum::response::Response> {
    let file = share_target(&s, &token).await?;
    let (body, ct) = s.storage.get(&file.object_key, file.encrypted).await?
        .ok_or(ApiError::NotFound)?;
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        ct.unwrap_or_else(|| "application/octet-stream".into()).parse().unwrap(),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", file.name).parse().unwrap(),
    );
    if let Some(etag) = &file.etag {
        h.insert(header::ETAG, format!("\"{etag}\"").parse().unwrap());
    }
    h.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    Ok((StatusCode::OK, h, body).into_response())
}

// -----------------------------------------------------------------------------
// Reports — TOR 4.15.15
// -----------------------------------------------------------------------------
pub async fn report_by_category(State(s): State<Arc<AppState>>) -> ApiResult<Json<Vec<CategoryReportRow>>> {
    Ok(Json(sqlx::query_as(
        r#"SELECT file_type AS category,
                  COUNT(*)::bigint AS file_count,
                  COALESCE(SUM(size_bytes), 0)::bigint AS size_bytes
             FROM files
            WHERE deleted_at IS NULL
            GROUP BY file_type
            ORDER BY size_bytes DESC"#
    ).fetch_all(&s.db).await?))
}

pub async fn report_by_time(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ReportTimeQuery>,
) -> ApiResult<Json<Vec<TimeReportRow>>> {
    let bucket = q.bucket.unwrap_or_else(|| "day".into());
    let bucket_expr = match bucket.as_str() {
        "day"   => "day",
        "month" => "month",
        "year"  => "year",
        _ => return Err(ApiError::BadRequest("bucket must be day|month|year".into())),
    };
    let limit = q.limit.unwrap_or(180).min(1000).max(1);
    let sql = format!(
        r#"SELECT date_trunc('{bucket_expr}', created_at) AS bucket,
                  COUNT(*)::bigint                          AS file_count,
                  COALESCE(SUM(size_bytes), 0)::bigint      AS size_bytes
             FROM files
            WHERE deleted_at IS NULL
            GROUP BY 1
            ORDER BY 1 DESC
            LIMIT $1"#
    );
    Ok(Json(sqlx::query_as(&sql).bind(limit).fetch_all(&s.db).await?))
}

// =============================================================================
// Workspace config — Q1: PATCH /api/workspace
// =============================================================================
/// Generic PATCH for workspace_config k/v rows.  Accepts any keys an admin
/// wants to update — we deliberately do NOT enforce a strict allow-list here
/// so adding a new tunable (e.g. `share_link_default_days`) is a one-line
/// frontend change with no migration.  Values are stored as TEXT; the
/// dashboard parses booleans/numbers itself.
#[derive(Debug, Deserialize)]
pub struct PatchWorkspace {
    pub values: std::collections::HashMap<String, String>,
}

pub async fn patch_workspace(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<PatchWorkspace>,
) -> ApiResult<Json<std::collections::HashMap<String, String>>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    for (k, v) in &body.values {
        if k.trim().is_empty() {
            return Err(ApiError::BadRequest("empty key".into()));
        }
        // UPSERT so missing keys are inserted on first write.
        sqlx::query(
            r#"INSERT INTO workspace_config (key, value, updated_at)
               VALUES ($1, $2, now())
               ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()"#
        )
        .bind(k).bind(v).execute(&s.db).await?;
    }
    // Echo back the full current state so the UI can update without a refetch.
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM workspace_config")
        .fetch_all(&s.db).await?;
    Ok(Json(rows.into_iter().collect()))
}

pub async fn list_workspace_config(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<std::collections::HashMap<String, String>>> {
    // Editors are allowed to read so the access policy section can render.
    crate::auth::require_role(&user.0, &["admin", "editor", "viewer"])?;
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM workspace_config")
        .fetch_all(&s.db).await?;
    Ok(Json(rows.into_iter().collect()))
}

// =============================================================================
// Members — Q2: users CRUD for the settings page
// =============================================================================
#[derive(Debug, Serialize, FromRow)]
pub struct MemberRow {
    pub id:           String,
    pub email:        String,
    pub display_name: String,
    pub avatar_tone:  String,
    pub role:         String,
    pub status:       String,
    pub quota_bytes:  i64,
    pub created_at:   DateTime<Utc>,
    /// Bytes the member has already uploaded — drives the per-user quota bar.
    pub used_bytes:   i64,
}

pub async fn list_members(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<MemberRow>>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let rows: Vec<MemberRow> = sqlx::query_as(
        r#"SELECT u.id, u.email, u.display_name, u.avatar_tone, u.role, u.status,
                  u.quota_bytes, u.created_at,
                  COALESCE((SELECT SUM(size_bytes) FROM files WHERE created_by = u.id AND deleted_at IS NULL), 0)::bigint AS used_bytes
             FROM users u
            ORDER BY u.display_name"#
    ).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct CreateMember {
    pub email:        String,
    pub display_name: String,
    pub password:     String,
    pub role:         Option<String>,
    pub avatar_tone:  Option<String>,
    pub quota_bytes:  Option<i64>,
}

pub async fn create_member(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<CreateMember>,
) -> ApiResult<Json<MemberRow>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    if body.email.trim().is_empty() || body.display_name.trim().is_empty() {
        return Err(ApiError::BadRequest("email + display_name required".into()));
    }
    if body.password.len() < 8 {
        return Err(ApiError::BadRequest("password must be ≥ 8 characters".into()));
    }
    let role = body.role.unwrap_or_else(|| "viewer".into());
    if !["admin", "editor", "viewer"].contains(&role.as_str()) {
        return Err(ApiError::BadRequest("role must be admin|editor|viewer".into()));
    }
    let tone = body.avatar_tone.unwrap_or_else(|| "slate".into());
    let quota = body.quota_bytes.unwrap_or(0).max(0);
    let id = format!("usr_{}", cuid2::create_id());
    let hash = crate::auth::hash_password(&body.password)?;

    sqlx::query(
        r#"INSERT INTO users
              (id, email, display_name, avatar_tone, role, password_hash, quota_bytes)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#
    )
    .bind(&id).bind(body.email.trim()).bind(body.display_name.trim())
    .bind(&tone).bind(&role).bind(&hash).bind(quota)
    .execute(&s.db).await
    .map_err(|e| {
        // Convert UNIQUE-violation into 400 so the UI can show a real message.
        match e {
            sqlx::Error::Database(d) if d.constraint() == Some("users_email_key") =>
                ApiError::BadRequest("email already exists".into()),
            other => ApiError::Db(other),
        }
    })?;

    let row: MemberRow = sqlx::query_as(
        r#"SELECT u.id, u.email, u.display_name, u.avatar_tone, u.role, u.status,
                  u.quota_bytes, u.created_at, 0::bigint AS used_bytes
             FROM users u WHERE id = $1"#
    ).bind(&id).fetch_one(&s.db).await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct PatchMember {
    pub display_name: Option<String>,
    pub role:         Option<String>,
    pub status:       Option<String>,
    pub avatar_tone:  Option<String>,
    pub quota_bytes:  Option<i64>,
}

pub async fn patch_member(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchMember>,
) -> ApiResult<Json<MemberRow>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    if let Some(r) = body.role.as_deref() {
        if !["admin", "editor", "viewer"].contains(&r) {
            return Err(ApiError::BadRequest("role must be admin|editor|viewer".into()));
        }
    }
    if let Some(st) = body.status.as_deref() {
        if !["active", "disabled"].contains(&st) {
            return Err(ApiError::BadRequest("status must be active|disabled".into()));
        }
    }
    // Belt-and-suspenders: refuse to lock out the last admin.
    if let Some(role_or_status) = body.role.as_deref().or(body.status.as_deref()) {
        let downgrading = body.role.as_deref() != Some("admin")
            || body.status.as_deref() == Some("disabled");
        if downgrading && role_or_status != "admin" {
            let cur_role: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
                .bind(&id).fetch_optional(&s.db).await?;
            if cur_role.as_deref() == Some("admin") {
                let other_admins: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = 'active' AND id <> $1"
                ).bind(&id).fetch_one(&s.db).await?;
                if other_admins == 0 {
                    return Err(ApiError::BadRequest("cannot change the only active admin".into()));
                }
            }
        }
    }

    sqlx::query(
        r#"UPDATE users SET
              display_name = COALESCE($2, display_name),
              role         = COALESCE($3, role),
              status       = COALESCE($4, status),
              avatar_tone  = COALESCE($5, avatar_tone),
              quota_bytes  = COALESCE($6, quota_bytes)
            WHERE id = $1"#
    )
    .bind(&id).bind(&body.display_name).bind(&body.role)
    .bind(&body.status).bind(&body.avatar_tone)
    .bind(body.quota_bytes.map(|q| q.max(0)))
    .execute(&s.db).await?;

    let row: MemberRow = sqlx::query_as(
        r#"SELECT u.id, u.email, u.display_name, u.avatar_tone, u.role, u.status,
                  u.quota_bytes, u.created_at,
                  COALESCE((SELECT SUM(size_bytes) FROM files WHERE created_by = u.id AND deleted_at IS NULL), 0)::bigint AS used_bytes
             FROM users u WHERE id = $1"#
    ).bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(row))
}

// =============================================================================
// Phase J — quota report
// =============================================================================
/// GET /api/quota — returns the nested cap/usage breakdown for the caller.
/// `?system_id=` and `?org_id=` narrow the report; without them only the
/// workspace + user scopes show up.
#[derive(serde::Deserialize)]
pub struct QuotaQuery {
    pub system_id: Option<String>,
    pub org_id:    Option<String>,
}

pub async fn quota_report(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<QuotaQuery>,
) -> ApiResult<Json<QuotaReport>> {
    let mut scopes = Vec::with_capacity(4);

    let ws_cap: i64 = sqlx::query_scalar::<_, String>(
        "SELECT value FROM workspace_config WHERE key = 'storage_quota_bytes'"
    ).fetch_optional(&s.db).await?
        .and_then(|v| v.parse().ok()).unwrap_or(0);
    let ws_used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE deleted_at IS NULL"
    ).fetch_one(&s.db).await?;
    scopes.push(QuotaScope { scope: "workspace", id: None, limit_bytes: ws_cap, used_bytes: ws_used });

    if let Some(sid) = q.system_id.as_deref() {
        let cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM systems WHERE id = $1")
            .bind(sid).fetch_optional(&s.db).await?.unwrap_or(0);
        let used: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE system_id = $1 AND deleted_at IS NULL"
        ).bind(sid).fetch_one(&s.db).await?;
        scopes.push(QuotaScope { scope: "system", id: Some(sid.to_string()), limit_bytes: cap, used_bytes: used });
    }
    if let Some(oid) = q.org_id.as_deref() {
        let cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM orgs WHERE id = $1")
            .bind(oid).fetch_optional(&s.db).await?.unwrap_or(0);
        let used: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE org_id = $1 AND deleted_at IS NULL"
        ).bind(oid).fetch_one(&s.db).await?;
        scopes.push(QuotaScope { scope: "org", id: Some(oid.to_string()), limit_bytes: cap, used_bytes: used });
    }
    let user_cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM users WHERE id = $1")
        .bind(&user.0.id).fetch_optional(&s.db).await?.unwrap_or(0);
    let user_used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE created_by = $1 AND deleted_at IS NULL"
    ).bind(&user.0.id).fetch_one(&s.db).await?;
    scopes.push(QuotaScope { scope: "user", id: Some(user.0.id.clone()), limit_bytes: user_cap, used_bytes: user_used });

    Ok(Json(QuotaReport { scopes }))
}

pub async fn patch_org_quota(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchOrgQuota>,
) -> ApiResult<Json<Org>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let q = body.quota_bytes.max(0);
    let row: Org = sqlx::query_as("UPDATE orgs SET quota_bytes = $2 WHERE id = $1 RETURNING *")
        .bind(&id).bind(q).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(row))
}

pub async fn patch_user_quota(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchUserQuota>,
) -> ApiResult<axum::http::StatusCode> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let q = body.quota_bytes.max(0);
    let n = sqlx::query("UPDATE users SET quota_bytes = $2 WHERE id = $1")
        .bind(&id).bind(q).execute(&s.db).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound); }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// =============================================================================
// Phase K — rotation HTTP surface
// =============================================================================
pub async fn list_rotation_policies(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<RotationPolicy>>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    Ok(Json(sqlx::query_as("SELECT * FROM rotation_policies ORDER BY scope_type, scope_id")
        .fetch_all(&s.db).await?))
}

pub async fn upsert_rotation_policy(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<UpsertRotationPolicy>,
) -> ApiResult<Json<RotationPolicy>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    if !["workspace","system","org","user"].contains(&body.scope_type.as_str()) {
        return Err(ApiError::BadRequest("scope_type must be workspace|system|org|user".into()));
    }
    if body.scope_type == "workspace" && body.scope_id.is_some() {
        return Err(ApiError::BadRequest("workspace scope must have null scope_id".into()));
    }
    if body.scope_type != "workspace" && body.scope_id.is_none() {
        return Err(ApiError::BadRequest("scoped policy needs scope_id".into()));
    }
    // INSERT … ON CONFLICT keyed on (scope_type, scope_id) via partial unique
    // indexes the migration laid down.  Pick a fresh id for new rows; the
    // RETURNING handles both insert and update equally.
    let new_id = format!("rot_{}", cuid2::create_id());
    let row: RotationPolicy = sqlx::query_as(
        r#"INSERT INTO rotation_policies
              (id, scope_type, scope_id, keep_last_n_versions, archive_after_days, delete_after_days, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, now())
           ON CONFLICT (scope_type, scope_id) WHERE scope_id IS NOT NULL DO UPDATE SET
              keep_last_n_versions = EXCLUDED.keep_last_n_versions,
              archive_after_days   = EXCLUDED.archive_after_days,
              delete_after_days    = EXCLUDED.delete_after_days,
              updated_at = now()
           RETURNING *"#
    )
    .bind(&new_id).bind(&body.scope_type).bind(&body.scope_id)
    .bind(body.keep_last_n_versions).bind(body.archive_after_days).bind(body.delete_after_days)
    .fetch_one(&s.db).await?;
    Ok(Json(row))
}

pub async fn delete_rotation_policy(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let n = sqlx::query("DELETE FROM rotation_policies WHERE id = $1")
        .bind(&id).execute(&s.db).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound); }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// POST /api/rotation/run — fire the rotation engine immediately.  Idempotent
/// + safe to call from a cron job or the admin UI.
pub async fn run_rotation_now(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let stats = crate::rotation::run_once(&s, Some(&user.0.id)).await
        .map_err(ApiError::Other)?;
    Ok(Json(serde_json::json!({
        "versions_pruned":    stats.versions_pruned,
        "files_archived":     stats.files_archived,
        "files_hard_deleted": stats.files_hard_deleted,
    })))
}

pub async fn list_rotation_runs(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<RotationRun>>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    Ok(Json(sqlx::query_as(
        "SELECT * FROM rotation_runs ORDER BY started_at DESC LIMIT 50"
    ).fetch_all(&s.db).await?))
}

// =============================================================================
// API keys — machine-to-machine credentials (admin-managed). A key
// authenticates AS `user_id`, inheriting that account's role + system access,
// so the existing require_role / ensure_system_access checks apply unchanged.
// See auth.rs (resolve_api_key) + migrations/0015_api_keys.sql.
// =============================================================================
#[derive(Debug, Deserialize)]
pub struct NewApiKey {
    pub name: String,
    pub user_id: String,
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ApiKeyOut {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub user_id: String,
    pub created_by: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CreatedApiKey {
    /// Plaintext key — shown ONCE. Store it now; only its hash is persisted.
    pub key: String,
    #[serde(flatten)]
    pub meta: ApiKeyOut,
}

const API_KEY_COLS: &str =
    "id, name, key_prefix, user_id, created_by, last_used_at, expires_at, revoked_at, created_at";

pub async fn list_api_keys(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<ApiKeyOut>>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    Ok(Json(sqlx::query_as(&format!(
        "SELECT {API_KEY_COLS} FROM api_keys ORDER BY created_at DESC"
    )).fetch_all(&s.db).await?))
}

pub async fn create_api_key(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<NewApiKey>,
) -> ApiResult<Json<CreatedApiKey>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name required".into()));
    }
    // The key authenticates AS this user — require an active account so its
    // role/access is well-defined.
    let target: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM users WHERE id = $1 AND status = 'active'",
    ).bind(&body.user_id).fetch_optional(&s.db).await?;
    if target.is_none() {
        return Err(ApiError::BadRequest("user_id must be an active user".into()));
    }

    let (plaintext, hash, prefix) = crate::auth::generate_api_key();
    let id = Uuid::now_v7();
    let expires_at = body.expires_in_days.filter(|d| *d > 0).map(|d| Utc::now() + Duration::days(d));

    let meta: ApiKeyOut = sqlx::query_as(&format!(
        r#"INSERT INTO api_keys (id, name, key_hash, key_prefix, user_id, created_by, expires_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING {API_KEY_COLS}"#
    ))
    .bind(id).bind(name).bind(&hash).bind(&prefix)
    .bind(&body.user_id).bind(&user.0.id).bind(expires_at)
    .fetch_one(&s.db).await?;

    // Activity trail (fire-and-forget; never block key creation on it).
    let _ = sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, system_id, created_at, actor_id)
           VALUES ($1, $2, $3, 'created API key', $4, NULL, NULL, now(), $5)"#
    )
    .bind(Uuid::now_v7()).bind(&user.0.display_name).bind(&user.0.avatar_tone)
    .bind(name).bind(&user.0.id)
    .execute(&s.db).await;

    Ok(Json(CreatedApiKey { key: plaintext, meta }))
}

pub async fn revoke_api_key(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let n = sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(id).execute(&s.db).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound); }
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// API docs — serve the OpenAPI spec (embedded at build time so it can't drift
// from the file) + a Swagger UI. Public, like /health: the spec contains no
// secrets and integrators should be able to read the API without a session.
// =============================================================================
pub async fn openapi_spec() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
        include_str!("../openapi.yaml"),
    )
}

pub async fn swagger_ui() -> axum::response::Html<&'static str> {
    // Swagger UI assets load from the unpkg CDN (dev/internal convenience —
    // no bundling). The spec lives at /fh/api/openapi.yaml (same origin).
    axum::response::Html(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>File Hub API — Swagger UI</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"/>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js" crossorigin></script>
  <script>
    window.ui = SwaggerUIBundle({
      url: "/fh/api/openapi.yaml",
      dom_id: "#swagger-ui",
      deepLinking: true,
    });
  </script>
</body>
</html>"##,
    )
}

// =============================================================================
// Unit tests — pure helpers (DB-touching tests live in tests/api.rs).
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    // Silence the unused-import warning for tests-only context.
    use crate::auth as _;

    #[test]
    fn detect_file_type_covers_tor_list() {
        for (name, want) in [
            ("contract.pdf",      "pdf"),
            ("memo.doc",          "docx"),
            ("memo.DOCX",         "docx"),
            ("budget.xls",        "xlsx"),
            ("slides.pptx",       "pptx"),
            ("page.html",         "html"),
            ("page.HTM",          "html"),
            ("feed.xml",          "xml"),
            ("audio.wav",         "wav"),
            ("song.mp3",          "mp3"),
            ("photo.jpg",         "img"),
            ("photo.JPEG",        "img"),
            ("photo.png",         "png"),
            ("anim.gif",          "img"),
            ("clip.mp4",          "mp4"),
            ("clip.MOV",          "mp4"),
            ("note.txt",          "txt"),
            ("README.md",         "md"),
            ("data.csv",          "csv"),
            ("data.json",         "json"),
            ("backup.zip",        "zip"),
            ("noextension",       "file"),
            ("weird.unknown",     "file"),
        ] {
            assert_eq!(detect_file_type(name), want, "for {name}");
        }
    }

    #[test]
    fn is_image_type_recognises_image_buckets() {
        for t in ["img", "png", "jpg", "jpeg", "gif"] {
            assert!(is_image_type(t), "{t} should be image");
        }
        for t in ["pdf", "docx", "mp4", "fold", ""] {
            assert!(!is_image_type(t), "{t} should NOT be image");
        }
    }

    #[test]
    fn random_token_is_url_safe_and_unique() {
        let a = random_token();
        let b = random_token();
        assert_ne!(a, b);
        assert!(a.len() >= 32);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn parse_uuid_rejects_garbage() {
        assert!(parse_uuid("not-a-uuid").is_err());
        assert!(parse_uuid("").is_err());
        assert!(parse_uuid("00000000-0000-4000-8000-000000000001").is_ok());
    }

    #[test]
    fn patch_file_folder_id_distinguishes_missing_and_null() {
        let p: PatchFile = serde_json::from_str("{}").unwrap();
        assert!(p.folder_id.is_none());
        let p: PatchFile = serde_json::from_str(r#"{"folder_id": null}"#).unwrap();
        assert!(matches!(p.folder_id, Some(None)));
        let uuid = "00000000-0000-4000-8000-000000006001";
        let p: PatchFile = serde_json::from_str(&format!(r#"{{"folder_id": "{uuid}"}}"#)).unwrap();
        match p.folder_id {
            Some(Some(u)) => assert_eq!(u.to_string(), uuid),
            other => panic!("expected Some(Some(uuid)), got {other:?}"),
        }
    }
}

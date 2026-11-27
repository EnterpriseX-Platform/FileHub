use std::sync::Arc;

use axum::{
    extract::{Multipart, Path, Query, State},
    response::IntoResponse,
    Json,
};
use axum::http::{header, HeaderMap, StatusCode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Datelike, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use rand::RngCore;
use crate::Id;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::models::*;
use crate::state::AppState;

/// Column list shared by every `files` read.  Keeps `search_tsv` (added in
/// migration 0004) out so sqlx::FromRow has nothing to map it to.
pub(crate) const FILE_COLS: &str = "id, name, file_type, size_bytes, system_id, org_id, bucket, object_key, project, status, owner, tags, version, metadata, etag, created_at, modified_at, folder_id, encrypted, deleted_at, created_by, category_code";

pub async fn health() -> &'static str { "ok" }

/// Deep readiness probe — verifies the DB pool can issue a query and the
/// storage backend can list its root.  Use this on Kubernetes
/// `readinessProbe` so a pod with a broken DB connection drops out of the
/// service mesh.  `/api/health` stays cheap for liveness.
pub async fn ready(State(s): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    // Answer within 3 s either way: a probe that hangs on a dead database is
    // as bad as no probe. Not ready = 503, so the pod leaves the load balancer.
    match tokio::time::timeout(std::time::Duration::from_secs(3), check_ready(&s)).await {
        Ok(Ok(v)) => Ok(v),
        _ => Err(ApiError::Busy("Not ready: the database or storage is not reachable.".into())),
    }
}

async fn check_ready(s: &AppState) -> ApiResult<Json<serde_json::Value>> {
    // 1. Database round-trip. Cast to bigint — a bare `SELECT 1` is INT4 in
    //    Postgres and decoding it as i64 trips a type mismatch (this probe is
    //    the k8s readinessProbe target, so a 500 here means pods never go
    //    Ready and the rollout hangs).
    let _: i64 = sqlx::query_scalar("SELECT 1::bigint").fetch_one(&s.db).await?;

    // 2. Storage round-trip — write + read + delete a tiny ephemeral blob.
    //    Keeps the probe at O(1) bytes; if the backend is encrypted, the
    //    cipher path is exercised too.
    let probe_key = format!(".healthz/probe-{}", crate::Id::new());
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
    let id     = crate::new_id();
    let bucket = body.bucket
        .as_deref()
        .map(|b| b.trim().to_lowercase())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| slugify_bucket(&body.name));

    // Storage keys and S3 prefixes are built from this — keep it to a safe,
    // S3-compatible charset (a-z 0-9 -, 2–63 chars).
    let valid = bucket.len() >= 2 && bucket.len() <= 63
        && bucket.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !bucket.starts_with('-');
    if !valid {
        return Err(ApiError::BadRequest(
            "bucket name must be 2-63 chars of a-z, 0-9 and '-'".into()));
    }

    // Block bucket-name collisions early so we don't leak partial state if
    // the unique index fires.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM systems WHERE bucket = $1)")
        .bind(&bucket).fetch_one(&s.db).await?;
    if exists {
        return Err(ApiError::BadRequest(format!("bucket '{bucket}' already exists")));
    }

    if body.vault && !s.storage.has_key() {
        return Err(ApiError::BadRequest("A Vault needs an encryption key on the server (STORAGE_ENC_KEY).".into()));
    }
    let tone        = body.tone.unwrap_or_else(|| "slate".into());
    let description = body.description;
    let quota       = body.quota_bytes.unwrap_or(0).max(0);

    let row: System = sqlx::query_as(
        r#"INSERT INTO systems
              (id, name, tone, bucket, status, description, system_type, quota_bytes, vault)
           VALUES ($1, $2, $3, $4, 'live', $5, 'shared', $6, $7)
           RETURNING *"#
    )
    .bind(&id).bind(&body.name).bind(&tone).bind(&bucket).bind(description).bind(quota).bind(body.vault)
    .fetch_one(&s.db).await?;

    // Audit row so admins can trace bucket creation through the activity feed.
    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, system_id, created_at, actor_id)
           VALUES ($1,$2,$3,'created bucket',$4,'system',$5,now(),$6)"#
    )
    .bind(crate::Id::new())
    .bind(&user.0.display_name).bind(&user.0.avatar_tone)
    .bind(&row.name).bind(&row.id).bind(&user.0.id)
    .execute(&s.db).await?;
    crate::basket::seed_defaults(&s.db, &row.id).await?;
    if row.vault {
        // Encrypted, no public links, and strict from the start: the creator manages it.
        sqlx::query("INSERT INTO basket_policies (scope_type, scope_id, encrypt, public_links) VALUES ('basket',$1,true,false) ON CONFLICT (scope_type, scope_id) DO UPDATE SET encrypt = true, public_links = false")
            .bind(&row.id).execute(&s.db).await?;
        sqlx::query("INSERT INTO acl_entries (id, system_id, resource_type, resource_id, principal, level, granted_by) VALUES ($1,$2,'basket',$2,$3,3,$4)")
            .bind(crate::Id::new()).bind(&row.id).bind(format!("user:{}", user.0.id)).bind(&user.0.id).execute(&s.db).await?;
    }
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
        return Err(ApiError::BadRequest(format!("Can't delete: this basket still has {alive} files. Move or delete them first.")));
    }
    sqlx::query("UPDATE systems SET deleted_at = now(), status = 'archived' WHERE id = $1")
        .bind(&id).execute(&s.db).await?;
    crate::basket::log_activity(&s.db, &user.0, "deleted basket", &cur.name, "basket", None, "").await;
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
        "UPDATE systems SET deleted_at = NULL
          WHERE id = (SELECT id FROM systems WHERE owner_user_id = $1 AND system_type = 'personal'
                       ORDER BY deleted_at NULLS FIRST LIMIT 1)
          RETURNING *"
    ).bind(&user.0.id).fetch_optional(&s.db).await?;
    if let Some(row) = existing {
        return Ok(Json(row));
    }
    let id     = crate::new_id();
    let bucket = format!("personal-{}", user.0.id);
    let row: System = sqlx::query_as(
        r#"INSERT INTO systems
              (id, name, tone, bucket, status, description, system_type, owner_user_id, quota_bytes)
           VALUES ($1, $2, $3, $4, 'live', $5, 'personal', $6, 0)
           RETURNING *"#
    )
    .bind(&id)
    .bind(format!("My Drive ({})", user.0.display_name))
    .bind(&user.0.avatar_tone)
    .bind(&bucket)
    .bind(format!("Personal drive for {}", user.0.email))
    .bind(&user.0.id)
    .fetch_one(&s.db).await?;
    crate::basket::seed_defaults(&s.db, &row.id).await?;
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
    if let Some(v) = q.category  { n += 1; where_sql.push_str(&format!(" AND category_code = ${n}")); str_binds.push(v); }
    if let Some(v) = q.tag.filter(|t| !t.trim().is_empty()) {
        n += 1;
        where_sql.push_str(&format!(" AND (CASE WHEN tags ~ '^\\s*\\[' THEN tags::jsonb ELSE '[]'::jsonb END) ? ${n}"));
        str_binds.push(v);
    }
    if let Some(v) = q.q.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        n += 1;
        where_sql.push_str(&format!(" AND (name ILIKE ${n} OR tags ILIKE ${n})"));
        str_binds.push(format!("%{}%", v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    }
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
        // Resource-level grants (acl.rs): principals, role, user id as arrays so
        // they bind after the other array params.
        let v = crate::acl::visibility(&s.db, &user.0, vec_binds.last().unwrap()).await?;
        n += 1; let bn = n; vec_binds.push(v.baskets);
        n += 1; let fln = n; vec_binds.push(v.folders);
        n += 1; let fin = n; vec_binds.push(v.files);
        n += 1; let un = n; vec_binds.push(vec![v.uid]);
        where_sql.push_str(&crate::acl::vis_sql("files", bn, fln, fin, un));
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
    Path(id): Path<Id>,
) -> ApiResult<Json<File>> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file.id, crate::acl::VIEW).await
        .map_err(|_| ApiError::NotFound)?;
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
pub(crate) struct UploadFields {
    pub(crate) name: Option<String>,
    pub(crate) system_id: Option<String>,
    pub(crate) org_id: Option<String>,
    pub(crate) folder_id: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) tags: Option<String>,
    /// Basket category code (basket_categories.code).
    pub(crate) category_code: Option<String>,
    /// Basket field values as a JSON object string (checked against basket_fields).
    pub(crate) fields: Option<String>,
    /// Integrations (S3) may leave required fields for people to fill in later.
    pub(crate) skip_required: bool,
    pub(crate) content_type: Option<String>,
    pub(crate) body: Option<bytes::Bytes>,
    /// Streamed upload already written to the staging area (preferred over
    /// `body` — large files never sit in memory).
    pub(crate) staged: Option<crate::storage::Staged>,
    /// The person ticked "encrypt this file" (only where the policy allows it).
    pub(crate) encrypt_file: bool,
    /// What the caller said about where the file came from (see origin.rs).
    pub(crate) origin: Option<crate::origin::Extra>,
    /// A person is uploading (web form, chunked upload, upload link): the
    /// default size limit applies where no policy sets one, as it does for TUS.
    pub(crate) person_upload: bool,
}

/// Refuse "encrypt this file" where the basket/folder policy doesn't offer it,
/// or when the server has no key for the kind of encryption the policy picks.
pub(crate) fn check_file_choice(s: &AppState, pol: &crate::basket::Policy) -> ApiResult<()> {
    if !pol.file_choice() {
        return Err(ApiError::BadRequest("Per-file encryption isn't enabled for this folder.".into()));
    }
    if pol.file_choice_asymmetric() && !s.storage.has_rsa_key() {
        return Err(ApiError::BadRequest("No RSA key is configured on the server, so asymmetric encryption isn't available.".into()));
    }
    if !pol.file_choice_asymmetric() && !s.storage.has_key() {
        return Err(ApiError::BadRequest("No encryption key is configured on the server (STORAGE_ENC_KEY).".into()));
    }
    Ok(())
}

/// Max bytes for one multipart upload (`UPLOAD_MAX_MB`, default 2048).
/// Uploads stream to disk now, so this is a policy limit, not a RAM limit.
pub fn upload_max_bytes() -> u64 {
    std::env::var("UPLOAD_MAX_MB").ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(2048) * 1024 * 1024
}

/// Stream one multipart file field into the staging area (encrypting as it
/// goes).  Memory stays O(chunk) whatever the file size.
pub(crate) async fn stage_field(
    s: &AppState,
    field: &mut axum::extract::multipart::Field<'_>,
    max_bytes: u64,
) -> Result<crate::storage::Staged, ApiError> {
    // A client that stops sending (lost network, closed tab, slowloris) is
    // dropped after UPLOAD_IDLE_TIMEOUT_SECS without a byte, not held forever.
    let idle = std::time::Duration::from_secs(crate::config::env_num("UPLOAD_IDLE_TIMEOUT_SECS", 60));
    let mut w = s.storage.begin_stage().await?;
    loop {
        match tokio::time::timeout(idle, field.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                if let Err(e) = w.write(&chunk).await { w.abort().await; return Err(e.into()); }
            }
            Ok(Ok(None)) => break,
            Ok(Err(e)) => { w.abort().await; return Err(multipart_error(e, max_bytes)); }
            Err(_) => { w.abort().await; return Err(ApiError::Timeout("No data arrived for a while, so the upload was stopped. Try again.".into())); }
        }
        if w.size() > max_bytes {
            w.abort().await;
            return Err(ApiError::PayloadTooLarge(format!("file exceeds {} MB", max_bytes / 1024 / 1024)));
        }
    }
    Ok(w.finish().await?)
}

/// Map a multipart read error — the body limit surfaces as a generic
/// "Error parsing multipart" 400, which hid the real reason (file too large).
pub(crate) fn multipart_error(e: axum::extract::multipart::MultipartError, max_bytes: u64) -> ApiError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::PayloadTooLarge(format!("file exceeds {} MB", max_bytes / 1024 / 1024))
    } else {
        ApiError::BadRequest(e.to_string())
    }
}

/// Text extraction / thumbnail / Office preview after an upload — runs in the
/// background so the upload returns as soon as the bytes are durable, and is
/// skipped for files above `P1_MAX_MB` (default 32) which would otherwise be
/// read fully into memory.
pub(crate) fn spawn_p1(s: &AppState, id: Id, file_type: String, name: String, object_key: String, encrypted: bool, size: u64) {
    let max = crate::config::env_num("P1_MAX_MB", 32) * 1024 * 1024;
    if size > max { return; }
    let db = s.db.clone();
    let storage = s.storage.clone();
    crate::jobs::spawn_indexing(async move {
        let Some((bytes, _)) = storage.get(&object_key, encrypted).await.ok().flatten() else { return };
        let (ft, b) = (file_type.clone(), bytes.clone());
        // A hostile PDF can make the parser spin; give up after a minute so the
        // indexing slot is freed (the stray thread finishes or dies on its own).
        if let Ok(Ok(Some(text))) = tokio::time::timeout(std::time::Duration::from_secs(60),
            tokio::task::spawn_blocking(move || crate::p1::extract_text_from(&ft, &b))).await {
            crate::p1::index_file_content(&db, id, &text).await;
        }
        crate::p1::index_thumbnail(&db, id, &file_type, &bytes).await;
        crate::p1::index_office_preview(&db, id, &file_type, &name, &bytes).await;
    });
}

#[cfg(test)]
fn parse_uuid(s: &str) -> Result<Id, ApiError> {
    crate::Id::parse_str(s).map_err(|_| ApiError::BadRequest(format!("invalid id: {s}")))
}

async fn read_one_upload(s: &AppState, multipart: &mut Multipart) -> Result<UploadFields, ApiError> {
    let mut f = UploadFields::default();
    while let Some(mut field) = multipart.next_field().await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                f.name = field.file_name().map(str::to_string);
                f.content_type = field.content_type().map(str::to_string);
                if let Some(old) = f.staged.take() { s.storage.discard(old).await; }
                f.staged = Some(stage_field(s, &mut field, upload_max_bytes()).await?);
            }
            "system_id" => f.system_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "org_id"    => f.org_id    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "folder_id" => f.folder_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "project"   => f.project   = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "status"    => f.status    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "owner"     => f.owner     = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "tags"      => f.tags      = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "category_code" => f.category_code = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "encrypt"   => f.encrypt_file = matches!(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?.trim(), "1" | "true" | "yes" | "on"),
            "fields"    => f.fields    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            _ => {}
        }
    }
    Ok(f)
}

/// Nested quota enforcement — workspace → system → org → user.  Every level
/// with a non-zero cap has to fit `used + incoming_bytes`, and the first one
/// that overflows wins.  A zero cap at any level means "no limit at this
/// scope; defer to the next level".  Anonymous uploads skip the user check.
/// A storage total for quota checks, cached per pod.
///
/// Summing `files` scans every row (~0.1 s at 100k files). Under thousands of
/// concurrent uploads, recomputing it whenever the cache expired made every
/// request sum at once and saturated Postgres. Now: the first caller computes
/// it; afterwards a stale value is returned at once while ONE background task
/// refreshes it (stale-while-revalidate). Quotas are a soft ceiling, so a value
/// up to ~30 s old is fine.
pub(crate) async fn cached_sum(db: &sqlx::PgPool, key: &str, sql: &'static str, bind: Option<&str>) -> Result<i64, ApiError> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    struct Entry { value: i64, at: Instant, refreshing: bool }
    static CACHE: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    const FRESH: Duration = Duration::from_secs(30);
    let cache = CACHE.get_or_init(Default::default);
    let k = format!("{key}|{}", bind.unwrap_or(""));

    let stale = {
        let mut c = cache.lock().unwrap();
        match c.get_mut(&k) {
            Some(e) if e.at.elapsed() < FRESH => return Ok(e.value),
            Some(e) if e.refreshing => return Ok(e.value),
            Some(e) => { e.refreshing = true; Some(e.value) }
            None => None,
        }
    };
    let compute = {
        let (db, bind) = (db.clone(), bind.map(str::to_string));
        async move {
            let mut q = sqlx::query_scalar::<_, i64>(sql);
            if let Some(b) = bind { q = q.bind(b); }
            q.fetch_one(&db).await
        }
    };
    let store = move |v: Result<i64, sqlx::Error>| {
        let mut c = cache.lock().unwrap();
        match v {
            Ok(value) => { c.insert(k.clone(), Entry { value, at: Instant::now(), refreshing: false }); Ok(value) }
            Err(e) => { if let Some(en) = c.get_mut(&k) { en.refreshing = false; } Err(e) }
        }
    };
    match stale {
        Some(v) => { tokio::spawn(async move { let _ = store(compute.await); }); Ok(v) }
        None => Ok(store(compute.await)?),
    }
}

pub(crate) async fn enforce_quota(
    s: &AppState,
    incoming_bytes: i64,
    system_id: Option<&str>,
    org_id: Option<&str>,
    user_id: Option<&str>,
) -> Result<(), ApiError> {
    // (label, id, cap query, usage query, cached?) — a zero/absent cap means no limit
    // at that level. Workspace/basket/agency totals scan many rows, so they come
    // from the per-pod cache; a person's total is small and indexed, so it's exact.
    const WS_CAP: &str = "SELECT NULLIF(value, '')::bigint FROM workspace_config WHERE key = 'storage_quota_bytes'";
    let scopes: [(&str, Option<&str>, &str, &'static str, bool); 4] = [
        ("workspace", Some(""), WS_CAP,
         "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE deleted_at IS NULL", true),
        ("basket", system_id, "SELECT quota_bytes FROM systems WHERE id = $1",
         "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE system_id = $1 AND deleted_at IS NULL", true),
        ("agency", org_id, "SELECT quota_bytes FROM orgs WHERE id = $1",
         "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE org_id = $1 AND deleted_at IS NULL", true),
        ("person", user_id, "SELECT quota_bytes FROM users WHERE id = $1",
         "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE created_by = $1 AND deleted_at IS NULL", false),
    ];
    for (label, id, cap_sql, used_sql, cached) in scopes {
        let Some(id) = id else { continue };
        // The workspace has no id: its queries take no parameter.
        let param = (label != "workspace").then_some(id);
        let mut cap_q = sqlx::query_scalar::<_, Option<i64>>(cap_sql);
        if let Some(p) = param { cap_q = cap_q.bind(p); }
        let cap = cap_q.fetch_optional(&s.db).await?.flatten().unwrap_or(0);
        if cap <= 0 { continue; }
        let used = if cached {
            cached_sum(&s.db, label, used_sql, param).await?
        } else {
            sqlx::query_scalar::<_, i64>(used_sql).bind(id).fetch_one(&s.db).await?
        };
        if used + incoming_bytes > cap {
            return Err(ApiError::PayloadTooLarge(format!(
                "The {label} storage limit is reached ({} of {} used).", fmt_bytes(used), fmt_bytes(cap)
            )));
        }
    }
    Ok(())
}

fn fmt_bytes(n: i64) -> String {
    let gb = n as f64 / 1_073_741_824.0;
    if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{:.0} MB", n as f64 / 1_048_576.0) }
}

/// Auto tags (`key:value`, grouped into "Tag folders" in the explorer):
/// system / org / fiscal-year / uploaded-by.  The fiscal-year convention is a
/// workspace setting (`fiscal_year_start_month`, `fiscal_year_era`) — e.g. the
/// Thai government year starts in October and is numbered in B.E. — and the
/// tag is skipped when the start month is 0 (default).
async fn auto_tags(s: &AppState, system: &System, org_id: Option<&str>, owner: &str) -> String {
    let mut tags: Vec<String> = vec![format!("system:{}", system.name)];
    if let Some(oid) = org_id {
        if let Ok(Some((name,))) = sqlx::query_as::<_, (String,)>("SELECT name FROM orgs WHERE id = $1")
            .bind(oid)
            .fetch_optional(&s.db)
            .await
        {
            tags.push(format!("org:{name}"));
        }
    }
    let cfg: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM workspace_config WHERE key IN ('fiscal_year_start_month','fiscal_year_era')",
    ).fetch_all(&s.db).await.unwrap_or_default();
    let get = |k: &str| cfg.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let start: u32 = get("fiscal_year_start_month").parse().unwrap_or(0);
    if (1..=12).contains(&start) {
        let now = Utc::now();
        // A fiscal year that starts after January is named after the year it ends in.
        let mut y = now.year() + if start > 1 && now.month() >= start { 1 } else { 0 };
        if get("fiscal_year_era").eq_ignore_ascii_case("BE") { y += 543; }
        tags.push(format!("fiscal-year:{y}"));
    }
    if owner != "Anonymous" {
        tags.push(format!("uploaded-by:{owner}"));
    }
    serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into())
}

/// Upload without requiring system_id/org_id, and tag the file automatically.
///
/// When system_id/org_id are not supplied, fall back to the user's defaults that
/// were remembered when they signed in through the edge
/// (users.default_system_id / default_org_id) — so users can upload into their
/// own organisation without knowing any system/org IDs.
/// A person uploading a name that already exists in the same folder gets a new
/// version of that file instead of a second file with the same name (TOR 4.15.7).
/// The folder's "keep versions" choice then decides whether the old bytes stay.
/// Integrations (/FileService, S3, upload links) keep their own behaviour.
/// The live file in that folder with the same name (any letter case), if any.
pub(crate) async fn same_name_file(s: &AppState, sys: &str, folder: Option<&str>, name: &str) -> ApiResult<Option<Id>> {
    let folder = folder.filter(|v| !v.is_empty());
    Ok(sqlx::query_scalar(
        "SELECT id FROM files WHERE system_id = $1 AND folder_id IS NOT DISTINCT FROM $2 AND lower(name) = lower($3)
           AND deleted_at IS NULL ORDER BY modified_at DESC LIMIT 1")
        .bind(sys).bind(folder).bind(name).fetch_optional(&s.db).await?)
}

pub(crate) async fn persist_or_version(s: &AppState, user: &crate::auth::User, mut f: UploadFields) -> ApiResult<File> {
    let same: Option<Id> = match (f.name.as_deref(), f.system_id.as_deref(), f.staged.is_some()) {
        (Some(n), Some(sys), true) => same_name_file(s, sys, f.folder_id.as_deref(), n).await?,
        _ => None,
    };
    match (same, f.staged.take()) {
        (Some(id), Some(st)) => commit_version_enc(s, user, id, st, f.name.clone(), None, None, f.encrypt_file).await,
        (_, st) => { f.staged = st; f.person_upload = true; persist_upload_for(s, Some(user.id.as_str()), f, Some(user)).await }
    }
}

pub(crate) async fn persist_upload_for(
    s: &AppState,
    actor: Option<&str>,
    mut f: UploadFields,
    user: Option<&crate::auth::User>,
) -> ApiResult<File> {
    if let Some(u) = user {
        if f.system_id.is_none() || f.org_id.is_none() {
            let d: Option<(Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT default_system_id, default_org_id FROM users WHERE id = $1",
            )
            .bind(&u.id)
            .fetch_optional(&s.db)
            .await?;
            if let Some((dsys, dorg)) = d {
                if f.system_id.is_none() { f.system_id = dsys; }
                if f.org_id.is_none() { f.org_id = dorg; }
            }
        }
        if f.owner.is_none() {
            f.owner = Some(u.display_name.clone());
        }
    }

    enum Src { Staged(crate::storage::Staged), Bytes(bytes::Bytes) }
    let src = match (f.staged.take(), f.body.take()) {
        (Some(st), _) => Src::Staged(st),
        (None, Some(b)) => Src::Bytes(b),
        (None, None) => return Err(ApiError::BadRequest("missing file body".into())),
    };
    let size_bytes = match &src { Src::Staged(st) => st.size as i64, Src::Bytes(b) => b.len() as i64 };

    let _step = crate::admission::Phase::Storing.enter();
    // Bytes are staged; take a write slot so a burst of uploads runs its
    // database phase in turn instead of all starving the pool at once.
    let _slot = match crate::admission::upload_slot().await {
        Ok(p) => p,
        Err(e) => { if let Src::Staged(st) = src { s.storage.discard(st).await; } return Err(e); }
    };
    // Validation before the bytes become visible — a staged upload that fails
    // here is discarded so the staging area doesn't fill up with orphans.
    let checked: Result<(String, String, System, crate::basket::Policy), ApiError> = async {
        let f = &mut f;
        let name = f.name.clone().ok_or_else(|| ApiError::BadRequest("missing file".into()))?;
        let system_id = f.system_id.clone().ok_or_else(|| ApiError::BadRequest("missing system_id".into()))?;
        let system: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1 AND deleted_at IS NULL")
            .bind(&system_id).fetch_optional(&s.db).await?
            .ok_or_else(|| ApiError::BadRequest("unknown system_id".into()))?;
        // A bucket switched off in settings ("Enabled (accept new files)") refuses new files.
        if system.status != "live" {
            return Err(ApiError::BadRequest(format!("bucket '{}' is not accepting new files", system.bucket)));
        }
        enforce_quota(s, size_bytes, Some(&system_id), f.org_id.as_deref(), actor).await?;
        // Basket platform: access to the destination, its policy, category and fields.
        if let Some(fid) = f.folder_id.as_deref().filter(|v| !v.is_empty()) {
            let fs: Option<(String,)> = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL")
                .bind(fid).fetch_optional(&s.db).await?;
            if fs.map(|(x,)| x != system_id).unwrap_or(true) {
                return Err(ApiError::BadRequest("folder_id is not in this basket".into()));
            }
        }
        if let Some(u) = user {
            crate::acl::require_folder(&s.db, u, &system_id, f.folder_id.as_deref().filter(|v| !v.is_empty()), crate::acl::EDIT).await
                .map_err(|e| if matches!(e, ApiError::NotFound) { ApiError::Forbidden } else { e })?;
        }
        let dest_folder = f.folder_id.as_deref().filter(|v| !v.is_empty());
        let pol = if f.person_upload {
            crate::basket::check_upload_policy(&s.db, &system_id, dest_folder, &name, size_bytes).await?
        } else {
            crate::basket::check_policy(&s.db, &system_id, dest_folder, &name, size_bytes).await?
        };
        if pol.encrypt == Some(true) && !s.storage.has_key() {
            return Err(ApiError::BadRequest("This folder requires encryption, but no encryption key is set up on the server (STORAGE_ENC_KEY).".into()));
        }
        crate::basket::check_category(&s.db, &system_id, f.category_code.as_deref()).await?;
        // FileHub's own content checks (real type, programs, malware).
        match &src {
            Src::Staged(st) => st.vet(&name, &pol.clean_ext().unwrap_or_default()).map_err(ApiError::BadRequest)?,
            Src::Bytes(b) => crate::scan::check_type(&name, &b[..b.len().min(crate::scan::HEAD)], &pol.clean_ext().unwrap_or_default()).map_err(ApiError::BadRequest)?,
        }
        let fv: serde_json::Value = match f.fields.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(raw) => serde_json::from_str(raw).map_err(|_| ApiError::BadRequest("fields must be a JSON object".into()))?,
            None => serde_json::Value::Null,
        };
        // Required fields are enforced for people; integrations may fill them later.
        let enforce = !f.skip_required && user.map(|u| u.role != "admin").unwrap_or(false) && f.fields.is_some();
        f.fields = Some(crate::basket::validate_fields(&s.db, &system_id, &fv, enforce).await?.to_string());
        if let Some(st) = f.status.as_deref() {
            let ok: Option<(String,)> = sqlx::query_as("SELECT code FROM basket_statuses WHERE system_id = $1 AND code = $2")
                .bind(&system_id).bind(st).fetch_optional(&s.db).await?;
            if ok.is_none() { return Err(ApiError::BadRequest(format!("Status \"{st}\" does not exist in this basket."))); }
        } else {
            f.status = crate::basket::initial_status(&s.db, &system_id).await?;
        }
        Ok((name, system_id, system, pol))
    }.await;
    let (name, system_id, system, pol) = match checked {
        Ok(v) => v,
        Err(e) => {
            if let Src::Staged(st) = src { s.storage.discard(st).await; }
            return Err(e);
        }
    };

    let id = crate::Id::new();
    let file_type = detect_file_type(&name);
    // Keep `name` in the DB so the user-facing display is preserved verbatim,
    // but rewrite the on-disk key through sanitize_filename so a malicious
    // filename can't traverse paths, inject quotes, or leave control bytes
    // in storage keys.
    let object_key = crate::mount::object_key_for(&s.db, &system, f.folder_id.as_deref(), id, &name).await?;
    let ct = f.content_type.clone()
        .or_else(|| mime_guess::from_path(&name).first().map(|m| m.to_string()));

    if f.encrypt_file { check_file_choice(s, &pol)?; }
    let want_enc = f.encrypt_file || pol.encrypt.unwrap_or_else(|| s.storage.encryption_enabled());
    let asym = pol.asymmetric() || (f.encrypt_file && pol.file_choice_asymmetric());
    let _ = &ct;
    let (etag, encrypted) = match src {
        Src::Staged(st) => s.storage.commit_mode(st, &object_key, want_enc || asym, asym).await?,
        Src::Bytes(b) => s.storage.put_mode(&object_key, b, want_enc || asym, asym).await?,
    };

    let owner = f.owner.unwrap_or_else(|| "Anonymous".into());
    let status = f.status.unwrap_or_else(|| "Draft".into());
    // No tags supplied = tag automatically from what we already know
    // (source system · organisation · fiscal year · uploader) so files stay searchable later.
    let tags_json = match f.tags {
        Some(t) if t.trim() != "" && t.trim() != "[]" => t,
        _ => auto_tags(s, &system, f.org_id.as_deref(), &owner).await,
    };
    let now = Utc::now();

    // File row and its "uploaded" activity in one statement = one commit,
    // one WAL flush per upload (matters at thousands of uploads at once).
    let file: File = sqlx::query_as(&format!(
        r#"WITH f AS (
             INSERT INTO files
               (id, name, file_type, size_bytes, system_id, org_id, folder_id, bucket, object_key,
                project, status, owner, tags, version, metadata, etag, encrypted, created_at, modified_at, created_by, category_code)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,1,$18,$14,$15,$16,$16,$17,$19)
             RETURNING *),
         a AS (
           INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           SELECT $20, $12, $21, 'uploaded', $2, $3, f.id, $5, $6, $16, $17 FROM f)
         SELECT {FILE_COLS} FROM f"#
    ))
    .bind(id).bind(&name).bind(&file_type).bind(size_bytes)
    .bind(&system_id).bind(&f.org_id).bind(&f.folder_id)
    .bind(&system.bucket).bind(&object_key)
    .bind(&f.project).bind(&status).bind(&owner).bind(&tags_json)
    .bind(&etag).bind(encrypted).bind(now).bind(actor)
    .bind(f.fields.clone().unwrap_or_else(|| "{}".into()))
    .bind(f.category_code.clone().filter(|c| !c.is_empty()))
    .bind(crate::Id::new()).bind(&system.tone)
    .fetch_one(&s.db).await?;

    crate::objectlock::apply_default(&s.db, id, &pol, actor).await?;

    // P1: extract text + thumbnail + Office → PDF preview, in the background.
    spawn_p1(s, id, file_type.clone(), name.clone(), object_key.clone(), encrypted, size_bytes as u64);

    crate::origin::record(&s.db, &file.id.to_string(), actor, f.origin.take().unwrap_or_default()).await;
    crate::webhooks::emit(&s.db, "file.uploaded", &file.system_id, crate::webhooks::file_data(&file)).await;
    Ok(file)
}

pub async fn upload_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Json<File>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    // Refuse over-limit callers before taking their bytes.
    { let _step = crate::admission::Phase::Checks.enter(); crate::auth::upload_rate_limit(&s.db, &user.0.id).await?; }
    let mut fields = { let _step = crate::admission::Phase::ReadingBody.enter(); read_one_upload(&s, &mut multipart).await? };
    fields.origin = Some(crate::origin::from_headers(&headers));
    if let Some(ref sid) = fields.system_id {
        let _step = crate::admission::Phase::Checks.enter();
        crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
    }
    Ok(Json(persist_or_version(&s, &user.0, fields).await?))
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
                    category_code: shared.category_code.clone(),
                    fields:    shared.fields.clone(),
                    encrypt_file: shared.encrypt_file,
                    ..Default::default()
                };
                one.name = field.file_name().map(str::to_string);
                one.content_type = field.content_type().map(str::to_string);
                one.staged = Some(stage_field(&s, &mut field, upload_max_bytes()).await?);
                if let Some(ref sid) = one.system_id {
                    crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
                }
                // Charge the rate limiter PER FILE — a batch of N files spends N
                // tokens, matching the per-file cap (charging once per request
                // would let a single token persist unlimited files).
                crate::auth::upload_rate_limit(&s.db, &user.0.id).await?;
                out.push(persist_or_version(&s, &user.0, one).await?);
            }
            "system_id" => shared.system_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "org_id"    => shared.org_id    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "folder_id" => shared.folder_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "project"   => shared.project   = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "status"    => shared.status    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "owner"     => shared.owner     = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "tags"      => shared.tags      = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "category_code" => shared.category_code = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "encrypt"   => shared.encrypt_file = matches!(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?.trim(), "1" | "true" | "yes" | "on"),
            "fields"    => shared.fields    = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            _ => {}
        }
    }

    if out.is_empty() {
        return Err(ApiError::BadRequest("no files provided".into()));
    }
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    /// Manage only: the original bytes even where downloads are stamped.
    pub original: Option<bool>,
    /// An earlier version's number (see /versions); absent = the current one.
    pub version: Option<i64>,
}

pub async fn download_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
    Query(dq): Query<DownloadQuery>,
    headers_in: HeaderMap,
) -> ApiResult<axum::response::Response> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    let level = crate::acl::require_file(&s.db, &user.0, file.id, crate::acl::VIEW).await
        .map_err(|_| ApiError::NotFound)?;

    // An earlier version: its own bytes, as they were stored.
    if let Some(v) = dq.version.filter(|v| *v != file.version) {
        let row: Option<(String, Option<String>, Option<bool>)> = sqlx::query_as(
            "SELECT object_key, etag, encrypted FROM file_versions WHERE file_id = $1 AND version = $2"
        ).bind(file.id).bind(v).fetch_optional(&s.db).await?;
        let (key, etag, enc) = row.ok_or_else(|| ApiError::Coded(StatusCode::NOT_FOUND, "not_found", format!("Version {v} of this file doesn't exist.")))?;
        let pol = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?;
        let stamp_on = pol.stamp_downloads == Some(true) || crate::basket::is_vault(&s.db, &file.system_id).await?;
        if stamp_on && level < crate::acl::MANAGE {
            return Err(ApiError::Denied("Files in this basket are watermarked on download. Only managers can download old versions.".into()));
        }
        crate::basket::log_vault_access(&s.db, &user.0, &file.system_id, file.id, &file.name, &format!("downloaded version {v}")).await;
        let (stem, ext) = match file.name.rfind('.') { Some(i) => (&file.name[..i], &file.name[i..]), None => (file.name.as_str(), "") };
        let vname = format!("{stem} (v{v}){ext}");
        return crate::serve::stream_object(&s, crate::serve::ServeOpts {
            key: &key, encrypted: enc.unwrap_or(file.encrypted), name: &vname, etag: etag.as_deref(),
            disposition: "attachment", cache_control: "private, no-store",
        }, &headers_in).await;
    }

    // Stamped download (policy stamp_downloads, always in a Vault): PDF and
    // Office files go out as a PDF carrying who/when/file/copy on every page.
    let pol = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?;
    let stamp_on = pol.stamp_downloads == Some(true) || crate::basket::is_vault(&s.db, &file.system_id).await?;
    let stampable = matches!(file.file_type.as_str(), "pdf" | "docx" | "xlsx" | "pptx");
    let wants_original = dq.original.unwrap_or(false) && level >= crate::acl::MANAGE;
    if stamp_on && stampable && !wants_original {
        return stamped_download(&s, &user.0, &file, false).await;
    }
    crate::basket::log_vault_access(&s.db, &user.0, &file.system_id, file.id, &file.name, if wants_original { "downloaded the original" } else { "downloaded" }).await;

    if let (Some(etag), Some(inm)) = (&file.etag, headers_in.get(header::IF_NONE_MATCH)) {
        if inm.to_str().ok().map(|v| v.trim_matches('"')) == Some(etag.as_str()) {
            let mut h = HeaderMap::new();
            h.insert(header::ETAG, format!("\"{etag}\"").parse().unwrap());
            return Ok((StatusCode::NOT_MODIFIED, h).into_response());
        }
    }

    // Video / audio play inline so `<video>`/`<audio>` can seek (206 +
    // Content-Range); everything else is an attachment.  The body streams
    // from storage — memory no longer scales with the file size.
    let inline = matches!(file.file_type.as_str(),
        "mp4" | "mov" | "webm" | "mp3" | "wav" | "img" | "png" | "jpg" | "jpeg" | "gif" | "pdf");
    // Cache policy unchanged from the buffered version: thumbnails/images are
    // immutable per version, everything else is not stored.
    let cache_control = if is_image_type(&file.file_type) {
        "private, max-age=86400, immutable"
    } else {
        "private, no-store"
    };
    crate::serve::stream_object(&s, crate::serve::ServeOpts {
        key: &file.object_key,
        encrypted: file.encrypted,
        name: &file.name,
        etag: file.etag.as_deref(),
        disposition: if inline { "inline" } else { "attachment" },
        cache_control,
    }, &headers_in).await
}


pub(crate) async fn stamped_download(s: &Arc<AppState>, user: &crate::auth::User, file: &File, inline: bool) -> ApiResult<axum::response::Response> {
    crate::jobs::fits_in_memory(file.size_bytes, "stamp")?;
    let pdf: Vec<u8> = if file.file_type == "pdf" {
        s.storage.get(&file.object_key, file.encrypted).await?.ok_or(ApiError::NotFound)?.0.to_vec()
    } else {
        let cached: Option<Vec<u8>> = sqlx::query_scalar("SELECT data FROM file_previews WHERE file_id = $1 AND mime = 'application/pdf'")
            .bind(file.id).fetch_optional(&s.db).await?;
        match cached {
            Some(p) => p,
            None => {
                let (body, _) = s.storage.get(&file.object_key, file.encrypted).await?.ok_or(ApiError::NotFound)?;
                let name = file.name.clone();
                crate::jobs::office_to_pdf(name, body.to_vec()).await
                    .ok_or_else(|| ApiError::BadRequest("This file must be downloaded with a stamp, but it couldn't be converted to PDF. Ask someone with Manage to download the original.".into()))?
            }
        }
    };
    let org: Option<String> = sqlx::query_scalar("SELECT o.code FROM users u JOIN orgs o ON o.id = u.default_org_id WHERE u.id = $1")
        .bind(&user.id).fetch_optional(&s.db).await?;
    let code = crate::stamp::copy_code();
    let now = Utc::now().with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap());
    let when = now.format("%d %b %Y %H:%M +07").to_string();
    let fid = file.id.to_string();
    let info_email = user.email.clone();
    let org_s = org.unwrap_or_default();
    let stamped = tokio::task::spawn_blocking(move || crate::stamp::stamp_pdf(&pdf, &crate::stamp::StampInfo {
        email: &info_email, when: &when, file_id: &fid, copy_code: &code, org: &org_s,
    }).map(|b| (b, code)))
        .await.map_err(|e| ApiError::Other(anyhow::anyhow!("stamp task: {e}")))?
        .ok_or_else(|| ApiError::BadRequest("This PDF couldn't be stamped, so it can't be downloaded here. Ask someone with Manage to download the original.".into()))?;
    let (bytes, code) = stamped;
    crate::basket::log_activity(&s.db, user, &format!("{} (stamped, copy {code})", if inline { "viewed" } else { "downloaded" }), &file.name, "file", Some(file.id), &file.system_id).await;
    let stem = file.name.rsplit_once('.').map(|(a, _)| a).unwrap_or(&file.name);
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, "application/pdf".parse().unwrap());
    h.insert(header::CONTENT_DISPOSITION, crate::serve::content_disposition(if inline { "inline" } else { "attachment" }, &format!("{stem}.pdf")));
    h.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    h.insert(axum::http::HeaderName::from_static("x-filehub-copy"), code.parse().unwrap());
    Ok((StatusCode::OK, h, bytes).into_response())
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
    Path(id): Path<Id>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<StatusCode> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    if !q.hard.unwrap_or(false) {
        trash_file(&s, &user.0, &file, "").await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    crate::auth::require_role(&user.0, &["admin"])?;
    crate::acl::require_file(&s.db, &user.0, id, crate::acl::EDIT).await?;
    crate::objectlock::guard_delete(&s.db, id, &user.0).await?;
    let version_keys: Vec<(String,)> = sqlx::query_as("SELECT object_key FROM file_versions WHERE file_id = $1")
        .bind(id).fetch_all(&s.db).await?;
    sqlx::query("DELETE FROM files WHERE id = $1").bind(id).execute(&s.db).await?;
    let _ = s.storage.delete(&file.object_key).await;
    for (key,) in version_keys { let _ = s.storage.delete(&key).await; }
    crate::basket::log_activity(&s.db, &user.0, "deleted for good", &file.name, &file.file_type, None, &file.system_id).await;
    crate::webhooks::emit(&s.db, "file.deleted", &file.system_id, serde_json::json!({ "id": file.id, "name": file.name, "permanent": true })).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Send a file to Trash — the one path every caller uses (web, S3, legacy):
/// Edit access, object lock, the update, the activity row and the webhook.
/// `via` is appended to the activity text (e.g. " via S3").
pub(crate) async fn trash_file(s: &AppState, user: &crate::auth::User, file: &File, via: &str) -> ApiResult<()> {
    let lvl = crate::acl::require_file(&s.db, user, file.id, crate::acl::EDIT).await?;
    // A status that locks content also protects the file from deletion, except
    // for people with Manage (who can change the status anyway).
    if lvl < crate::acl::MANAGE && crate::basket::content_locked(&s.db, &file.system_id, &file.status).await? {
        return Err(ApiError::ContentLocked("This file's status locks it. Someone with Manage access can change the status or delete it.".into()));
    }
    crate::objectlock::guard_delete(&s.db, file.id, user).await?;
    sqlx::query("UPDATE files SET deleted_at = now(), modified_at = now() WHERE id = $1").bind(file.id).execute(&s.db).await?;
    crate::basket::log_activity(&s.db, user, &format!("sent to trash{via}"), &file.name, &file.file_type, Some(file.id), &file.system_id).await;
    crate::webhooks::emit(&s.db, "file.deleted", &file.system_id, serde_json::json!({ "id": file.id, "name": file.name, "permanent": false })).await;
    Ok(())
}

/// Trash: what the caller could restore — everything for admins, otherwise
/// files they uploaded or have Edit on (the same rule as restore).
pub async fn list_trash(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Vec<File>>> {
    if user.0.role == "admin" {
        return Ok(Json(sqlx::query_as(&format!(
            "SELECT {FILE_COLS} FROM files WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC LIMIT 500"
        )).fetch_all(&s.db).await?));
    }
    if user.0.role == "viewer" { return Ok(Json(vec![])); }
    let p = crate::acl::principals(&s.db, &user.0).await?;
    // Only baskets this person can open: shared baskets and their own drive —
    // never someone else's personal drive, whatever its (empty) access list says.
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?.unwrap_or_default();
    let rows: Vec<File> = sqlx::query_as(&format!(
        "SELECT {FILE_COLS} FROM files WHERE deleted_at IS NOT NULL
            AND system_id = ANY($4)
            AND (created_by = $3 OR fh_level($1, $2, system_id, folder_id, id, created_by, $3) >= 2)
          ORDER BY deleted_at DESC LIMIT 500"
    )).bind(&p).bind(&user.0.role).bind(&user.0.id).bind(&scope).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

pub async fn restore_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
) -> ApiResult<Json<File>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    let file: Option<File> = sqlx::query_as(&format!(
        "SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NOT NULL"
    )).bind(id).fetch_optional(&s.db).await?;
    let file = file.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await?;
    crate::acl::require_file(&s.db, &user.0, file.id, crate::acl::EDIT).await?;

    sqlx::query("UPDATE files SET deleted_at = NULL, modified_at = now() WHERE id = $1")
        .bind(id).execute(&s.db).await?;
    // Its folder went to Trash with it: bring the folder chain back too.
    if let Some(fld) = file.folder_id.as_deref() {
        sqlx::query(
            r#"WITH RECURSIVE up(id, parent_id, deleted_at) AS (
                 SELECT id, parent_id, deleted_at FROM folders WHERE id = $1
                 UNION ALL SELECT f.id, f.parent_id, f.deleted_at FROM folders f JOIN up ON f.id = up.parent_id WHERE up.deleted_at IS NOT NULL)
               UPDATE folders SET deleted_at = NULL WHERE id IN (SELECT id FROM up WHERE deleted_at IS NOT NULL)"#)
            .bind(fld).execute(&s.db).await?;
    }
    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate','restored',$3,$4,$5,$6,$7,now(),$8)"#,
    )
    .bind(crate::Id::new()).bind(&user.0.display_name)
    .bind(&file.name).bind(&file.file_type).bind(file.id)
    .bind(&file.system_id).bind(&file.org_id).bind(&user.0.id)
    .execute(&s.db).await?;

    let restored: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1")).bind(id).fetch_one(&s.db).await?;
    crate::webhooks::emit(&s.db, "file.restored", &restored.system_id, crate::webhooks::file_data(&restored)).await;
    Ok(Json(restored))
}

pub async fn list_activity(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<ActivityQuery>,
) -> ApiResult<Json<Vec<Activity>>> {
    let limit  = q.limit.unwrap_or(50).min(500).max(1);
    let offset = q.offset.unwrap_or(0).max(0);
    // Filters only when given; non-admins only see baskets they can reach (file names leak otherwise).
    let mut sql = String::from("SELECT * FROM activity WHERE true");
    let mut strs: Vec<String> = Vec::new();
    let mut n = 2usize;
    if let Some(t) = q.q.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND (actor ILIKE ${n} OR action ILIKE ${n} OR target ILIKE ${n})"));
        strs.push(format!("%{}%", t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    }
    if let Some(sys) = q.system_id.as_deref().filter(|v| !v.is_empty()) {
        n += 1; sql.push_str(&format!(" AND system_id = ${n}")); strs.push(sys.to_string());
    }
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;
    // …and within those, only rows about files they may see.
    let mut vecs: Vec<Vec<String>> = Vec::new();
    if let Some(ids) = &scope {
        n += 1; sql.push_str(&format!(" AND system_id = ANY(${n})")); vecs.push(ids.clone());
        let v = crate::acl::visibility(&s.db, &user.0, ids).await?;
        let (bn, fln, fin, un) = (n + 1, n + 2, n + 3, n + 4);
        n += 4;
        sql.push_str(&format!(" AND (activity.file_id IS NULL OR activity.file_id IN (SELECT files.id FROM files WHERE true{}))", crate::acl::vis_sql("files", bn, fln, fin, un)));
        vecs.extend([v.baskets, v.folders, v.files, vec![v.uid]]);
    }
    let _ = n;
    sql.push_str(" ORDER BY created_at DESC LIMIT $1 OFFSET $2");
    let mut qy = sqlx::query_as::<_, Activity>(&sql).bind(limit).bind(offset);
    for v in &strs { qy = qy.bind(v); }
    for v in &vecs { qy = qy.bind(v); }
    let rows: Vec<Activity> = qy.fetch_all(&s.db).await?;
    Ok(Json(rows))
}

/// Activity of one file (side panel → Activity).
pub async fn file_activity(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
) -> ApiResult<Json<Vec<Activity>>> {
    crate::acl::require_file(&s.db, &user.0, id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    Ok(Json(sqlx::query_as("SELECT * FROM activity WHERE file_id = $1 ORDER BY created_at DESC LIMIT 100")
        .bind(id).fetch_all(&s.db).await?))
}

/// All tags in use, with file counts — feeds the "Tag folders" tree in the
/// explorer.  A tag written as `key:value` is split so the UI can group by key
/// (e.g. `fiscal-year` → `2569`, `2570`).  Scoped to the buckets the caller
/// can see (personal drives of other users are excluded).
pub async fn list_tags(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<crate::models::TagsQuery>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;
    // Non-admins count only files they may see (tags would leak names otherwise).
    let (vis, vis_sql) = match &scope {
        Some(ids) => (Some(crate::acl::visibility(&s.db, &user.0, ids).await?), crate::acl::vis_sql("f", 4, 5, 6, 7)),
        None => (None, String::new()),
    };
    let sql = format!(
        r#"SELECT t, COUNT(*)::bigint
             FROM files f,
                  jsonb_array_elements_text(CASE WHEN f.tags ~ '^\s*\[' THEN f.tags::jsonb ELSE '[]'::jsonb END) t
            WHERE f.deleted_at IS NULL
              AND ($1::text IS NULL OR f.system_id = $1)
              AND ($2::text[] IS NULL OR f.system_id = ANY($2)){vis_sql}
            GROUP BY t
            ORDER BY COUNT(*) DESC, t
            LIMIT $3"#);
    let mut qy = sqlx::query_as::<_, (String, i64)>(&sql).bind(q.system_id).bind(scope).bind(limit);
    if let Some(v) = vis { qy = qy.bind(v.baskets).bind(v.folders).bind(v.files).bind(vec![v.uid]); }
    let rows = qy.fetch_all(&s.db).await?;
    Ok(Json(rows.into_iter().map(|(tag, count)| {
        let (key, value) = match tag.split_once(':') {
            Some((k, v)) if !k.trim().is_empty() && !v.trim().is_empty() => (k.trim().to_string(), v.trim().to_string()),
            _ => (String::new(), tag.clone()),
        };
        serde_json::json!({ "tag": tag, "key": key, "value": value, "count": count })
    }).collect()))
}

/// Delete a saved view / tag folder — its creator or an admin.
pub async fn delete_view(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let owner: Option<Option<String>> = sqlx::query_scalar("SELECT created_by FROM views WHERE id = $1")
        .bind(&id).fetch_optional(&s.db).await?;
    let Some(owner) = owner else { return Err(ApiError::NotFound) };
    if user.0.role != "admin" && owner.as_deref() != Some(user.0.id.as_str()) {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("DELETE FROM views WHERE id = $1").bind(&id).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Saved views: your own and the shared ones (no creator).
pub async fn list_views(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Vec<View>>> {
    Ok(Json(sqlx::query_as("SELECT * FROM views WHERE created_by IS NULL OR created_by = $1 ORDER BY pinned DESC, created_at DESC")
        .bind(&user.0.id).fetch_all(&s.db).await?))
}

pub async fn create_view(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(v): Json<CreateView>,
) -> ApiResult<Json<View>> {
    // Views are a system table → CUID2 PK with `vw_` prefix.
    let id = crate::new_id();
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
    Path(file_id): Path<Id>,
) -> ApiResult<Json<Vec<Permission>>> {
    // Missing file → empty list (preserve the existing "permissions for
    // unknown id is just empty" semantic that the test suite locks in).
    // Access denial still returns an empty list — same observable behaviour
    // as a missing file so we don't leak existence.
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    if let Some((system_id,)) = row {
        if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err()
            || crate::acl::file_level(&s.db, &user.0, file_id).await.unwrap_or(0) < crate::acl::VIEW {
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
) -> ApiResult<(axum::http::HeaderMap, Json<Vec<File>>)> {
    let limit = q.limit.unwrap_or(50).min(500).max(1);
    let offset = q.offset.unwrap_or(0).max(0);
    let term = q.q.trim().to_string();
    let creator = q.creator.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let types: Vec<String> = q.types.as_deref().unwrap_or("").split(',').map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect();
    let date = |v: &Option<String>| -> ApiResult<Option<chrono::NaiveDate>> {
        match v.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            None => Ok(None),
            Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map(Some).map_err(|_| ApiError::BadRequest("Dates must be in YYYY-MM-DD format.".into())),
        }
    };
    let (from, to) = (date(&q.from)?, date(&q.to)?);
    let any_filter = creator.is_some() || !types.is_empty() || from.is_some() || to.is_some() || q.file_type.is_some() || q.status.is_some();
    if term.is_empty() && !any_filter {
        return Err(ApiError::BadRequest("q must not be empty".into()));
    }

    // If the caller asked for a specific system, verify they can see it.
    if let Some(ref sid) = q.system_id {
        crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
    }
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;

    let like = |t: &str| format!("%{}%", t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut str_binds: Vec<String> = Vec::new();
    let mut vec_binds: Vec<Vec<String>> = Vec::new();
    let mut date_binds: Vec<chrono::NaiveDate> = Vec::new();
    let mut n = 0usize;
    let mut sql = format!("SELECT {FILE_COLS} FROM files WHERE deleted_at IS NULL");
    if !term.is_empty() {
        // One indexed lookup per source, UNIONed: an OR across columns can't use the
        // trigram/full-text indexes and made Postgres scan every file per search.
        // Owner covers the name shown on the file and the uploader's account (name, e-mail).
        // Substring search inside file text needs the trigram index, which only
        // works for 3+ characters; shorter terms would read every document's
        // text on each search, so they match whole words (full-text index) only.
        let content_like = if term.chars().count() >= 3 { "            UNION SELECT file_id FROM file_content WHERE content ILIKE $2 " } else { "" };
        sql.push_str(&format!(" AND id IN ( \
            SELECT id FROM files WHERE search_tsv @@ plainto_tsquery('simple', $1) \
            UNION SELECT id FROM files WHERE name ILIKE $2 \
            UNION SELECT id FROM files WHERE tags ILIKE $2 \
            UNION SELECT id FROM files WHERE owner ILIKE $2 \
            UNION SELECT id FROM files WHERE metadata ILIKE $2 \
            UNION SELECT f.id FROM files f JOIN users u ON u.id = f.created_by WHERE u.email ILIKE $2 OR u.display_name ILIKE $2 \
            UNION SELECT file_id FROM file_content WHERE content_tsv @@ plainto_tsquery('simple', $1) \
            {content_like}\
            UNION SELECT file_id FROM file_origin WHERE source_user ILIKE $2 OR ref_no ILIKE $2 OR screen ILIKE $2 OR org_code ILIKE $2 OR source_system ILIKE $2 OR connector_name ILIKE $2 \
         )"));
        str_binds.push(term.clone());
        str_binds.push(like(&term));
        n = 2;
    }
    if let Some(v) = q.system_id { n += 1; sql.push_str(&format!(" AND system_id = ${n}")); str_binds.push(v); }
    if let Some(v) = q.file_type { n += 1; sql.push_str(&format!(" AND file_type = ${n}")); str_binds.push(v); }
    if let Some(v) = q.status    { n += 1; sql.push_str(&format!(" AND status = ${n}"));    str_binds.push(v); }
    if let Some(c) = &creator {
        n += 1;
        sql.push_str(&format!(" AND (owner ILIKE ${n} OR created_by IN (SELECT id FROM users WHERE display_name ILIKE ${n} OR email ILIKE ${n}))"));
        str_binds.push(like(c));
    }
    // Vector binds and date binds come after the strings (bind order = position).
    if !types.is_empty() { n += 1; sql.push_str(&format!(" AND lower(file_type) = ANY(${n})")); vec_binds.push(types); }
    // Non-admins are limited to systems they belong to (shared + own personal).
    if let Some(scope_ids) = scope {
        n += 1;
        sql.push_str(&format!(" AND system_id = ANY(${n})"));
        vec_binds.push(scope_ids);
        let v = crate::acl::visibility(&s.db, &user.0, vec_binds.last().unwrap()).await?;
        n += 1; let bn = n; vec_binds.push(v.baskets);
        n += 1; let fln = n; vec_binds.push(v.folders);
        n += 1; let fin = n; vec_binds.push(v.files);
        n += 1; let un = n; vec_binds.push(vec![v.uid]);
        sql.push_str(&crate::acl::vis_sql("files", bn, fln, fin, un));
    }
    if let Some(d) = from { n += 1; sql.push_str(&format!(" AND created_at >= (${n}::date)::timestamp AT TIME ZONE 'Asia/Bangkok'")); date_binds.push(d); }
    if let Some(d) = to   { n += 1; sql.push_str(&format!(" AND created_at < ((${n}::date) + 1)::timestamp AT TIME ZONE 'Asia/Bangkok'")); date_binds.push(d); }
    // Total matching rows (for pagination) — same filters, no order/limit/offset.
    let count_sql = format!("SELECT COUNT(*) FROM ({sql}) t");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &str_binds { cq = cq.bind(b); }
    for b in &vec_binds { cq = cq.bind(b); }
    for b in &date_binds { cq = cq.bind(b); }
    // One read-only transaction for both queries: file_content rows hold whole
    // documents, so the planner wrongly prefers a full scan over the trigram /
    // full-text indexes (300 ms vs 2 ms). Prefer indexes for this search only.
    let mut tx = s.db.begin().await?;
    sqlx::query("SET LOCAL enable_seqscan = off").execute(&mut *tx).await?;
    let total: i64 = cq.fetch_one(&mut *tx).await?;

    n += 1; let limit_n = n;
    n += 1; let offset_n = n;
    sql.push_str(&format!(" ORDER BY modified_at DESC, id DESC LIMIT ${limit_n} OFFSET ${offset_n}"));

    let mut query = sqlx::query_as::<_, File>(&sql);
    for b in &str_binds { query = query.bind(b); }
    for b in &vec_binds { query = query.bind(b); }
    for b in &date_binds { query = query.bind(b); }
    query = query.bind(limit).bind(offset);

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::HeaderName::from_static("x-total-count"),
        axum::http::HeaderValue::from_str(&total.to_string()).unwrap(),
    );
    let rows = query.fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok((headers, Json(rows)))
}

#[derive(Debug, Deserialize)]
pub struct FolderSearchQuery {
    #[serde(default)]
    pub q: String,
    pub creator: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FolderHit {
    pub id: String,
    pub system_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub owner: String,
    pub created_at: DateTime<Utc>,
}

/// Folders whose name matches (search results list folders as well as files).
pub async fn search_folders(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<FolderSearchQuery>,
) -> ApiResult<Json<Vec<FolderHit>>> {
    let term = q.q.trim().to_string();
    if term.is_empty() { return Ok(Json(vec![])); }
    let limit = q.limit.unwrap_or(50).min(200).max(1);
    let like = |t: &str| format!("%{}%", t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut sql = String::from("SELECT id, system_id, parent_id, name, owner, created_at FROM folders WHERE deleted_at IS NULL AND name ILIKE $1");
    let mut strs = vec![like(&term)];
    let mut n = 1usize;
    if let Some(c) = q.creator.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND (owner ILIKE ${n} OR created_by IN (SELECT id FROM users WHERE display_name ILIKE ${n} OR email ILIKE ${n}))"));
        strs.push(like(c));
    }
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;
    let mut vecs: Vec<Vec<String>> = Vec::new();
    if let Some(scope_ids) = scope {
        let v = crate::acl::visibility(&s.db, &user.0, &scope_ids).await?;
        n += 1; sql.push_str(&format!(" AND system_id = ANY(${n})")); vecs.push(scope_ids);
        n += 1; sql.push_str(&format!(" AND (id = ANY(${n})")); vecs.push(v.folders);
        n += 1; sql.push_str(&format!(" OR created_by = (${n})[1])")); vecs.push(vec![v.uid]);
    }
    let mut dates = Vec::new();
    for (v, op) in [(&q.from, ">="), (&q.to, "<")] {
        if let Some(d) = v.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            let d = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| ApiError::BadRequest("Dates must be in YYYY-MM-DD format.".into()))?;
            n += 1;
            let plus = if op == "<" { " + 1" } else { "" };
            sql.push_str(&format!(" AND created_at {op} ((${n}::date){plus})::timestamp AT TIME ZONE 'Asia/Bangkok'"));
            dates.push(d);
        }
    }
    n += 1;
    sql.push_str(&format!(" ORDER BY name LIMIT ${n}"));
    let mut qy = sqlx::query_as::<_, FolderHit>(&sql);
    for b in &strs { qy = qy.bind(b); }
    for b in &vecs { qy = qy.bind(b); }
    for b in &dates { qy = qy.bind(b); }
    Ok(Json(qy.bind(limit).fetch_all(&s.db).await?))
}

// -----------------------------------------------------------------------------
// Versioning — TOR 4.15.7
// -----------------------------------------------------------------------------
pub async fn list_versions(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
) -> ApiResult<Json<Vec<FileVersion>>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.db).await?;
    let (_system_id,) = row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, id, crate::acl::VIEW).await
        .map_err(|_| ApiError::NotFound)?;
    let mut rows: Vec<FileVersion> = sqlx::query_as(
        "SELECT * FROM file_versions WHERE file_id = $1 ORDER BY version DESC"
    ).bind(id).fetch_all(&s.db).await?;
    // The current version first, with its own note and uploader.
    let cur: Option<(i64, String, i64, Option<String>, Option<String>, Option<String>, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT version, object_key, size_bytes, etag, version_note, version_by, owner, modified_at FROM files WHERE id = $1"
    ).bind(id).fetch_optional(&s.db).await?;
    if let Some((v, key, size, etag, note, by, owner, at)) = cur {
        rows.retain(|r| r.version != v);
        rows.insert(0, FileVersion { id, file_id: id, version: v, object_key: key, size_bytes: size, etag,
            uploaded_by: by.unwrap_or(owner), note, created_at: at });
    }
    Ok(Json(rows))
}

pub async fn upload_version(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
    mut multipart: Multipart,
) -> ApiResult<Json<File>> {
    check_version_target(&s, &user.0, id).await?;
    let mut staged: Option<crate::storage::Staged> = None;
    let mut filename: Option<String> = None;
    let mut uploaded_by: Option<String> = None;
    let mut note: Option<String> = None;
    let read: Result<(), ApiError> = async {
        while let Some(mut field) = multipart.next_field().await.map_err(|e| multipart_error(e, upload_max_bytes()))? {
            match field.name().unwrap_or("") {
                "file" => {
                    filename = field.file_name().map(str::to_string);
                    if let Some(old) = staged.take() { s.storage.discard(old).await; }
                    staged = Some(stage_field(&s, &mut field, upload_max_bytes()).await?);
                }
                "uploaded_by" => uploaded_by = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
                "note"        => note        = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
                _ => {}
            }
        }
        Ok(())
    }.await;
    if let Err(e) = read { if let Some(st) = staged { s.storage.discard(st).await; } return Err(e); }
    let staged = staged.ok_or_else(|| ApiError::BadRequest("missing file body".into()))?;
    Ok(Json(commit_version(&s, &user.0, id, staged, filename, uploaded_by, note).await?))
}

/// Edit on the file and a status that doesn't lock its content.
pub(crate) async fn check_version_target(s: &AppState, user: &crate::auth::User, id: Id) -> ApiResult<File> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, user, id, crate::acl::EDIT).await?;
    if crate::basket::content_locked(&s.db, &file.system_id, &file.status).await? {
        return Err(ApiError::ContentLocked("This file's status locks its content. Change the status before uploading a new version.".into()));
    }
    Ok(file)
}

/// Make already-staged bytes the next version of a file (multipart and chunked
/// uploads both end here). Discards the staged bytes on any refusal.
pub(crate) async fn commit_version(
    s: &AppState, user: &crate::auth::User, id: Id, staged: crate::storage::Staged,
    filename: Option<String>, uploaded_by: Option<String>, note: Option<String>,
) -> ApiResult<File> {
    commit_version_enc(s, user, id, staged, filename, uploaded_by, note, false).await
}

/// [`commit_version`] where the person may have ticked "encrypt this file".
#[allow(clippy::too_many_arguments)]
pub(crate) async fn commit_version_enc(
    s: &AppState, user: &crate::auth::User, id: Id, staged: crate::storage::Staged,
    filename: Option<String>, uploaded_by: Option<String>, note: Option<String>, encrypt_file: bool,
) -> ApiResult<File> {
    let _slot = match crate::admission::upload_slot().await {
        Ok(p) => p,
        Err(e) => { s.storage.discard(staged).await; return Err(e); }
    };
    let checked: ApiResult<(File, (bool, bool))> = async {
        let file = check_version_target(s, user, id).await?;
        let display_name = filename.clone().unwrap_or_else(|| file.name.clone());
        enforce_quota(s, staged.size as i64, Some(&file.system_id), file.org_id.as_deref(), Some(&user.id)).await?;
        let pol = crate::basket::check_upload_policy(&s.db, &file.system_id, file.folder_id.as_deref(), &display_name, staged.size as i64).await?;
        staged.vet(&display_name, &pol.clean_ext().unwrap_or_default()).map_err(ApiError::BadRequest)?;
        if encrypt_file { check_file_choice(s, &pol)?; }
        let file_asym = encrypt_file && pol.file_choice_asymmetric();
        let want_enc = encrypt_file || file.encrypted || pol.encrypt.unwrap_or_else(|| s.storage.encryption_enabled()) || pol.asymmetric();
        Ok((file, (want_enc, pol.asymmetric() || file_asym)))
    }.await;
    let (file, (want_enc, asym)) = match checked { Ok(v) => v, Err(e) => { s.storage.discard(staged).await; return Err(e); } };
    let keep_versions = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?
        .keep_versions.unwrap_or(true);
    let (cur_note, cur_by): (Option<String>, Option<String>) = sqlx::query_as("SELECT version_note, version_by FROM files WHERE id = $1")
        .bind(file.id).fetch_one(&s.db).await?;
    let uploaded_by = uploaded_by.unwrap_or_else(|| user.display_name.clone());
    let new_version = file.version + 1;
    let display_name = filename.unwrap_or_else(|| file.name.clone());
    let safe = sanitize_filename(&display_name);
    let (old_key, new_object_key) = match crate::mount::version_keys(s, &file.object_key, &file.bucket, file.id, file.version, new_version, &safe).await {
        Ok(k) => k,
        Err(e) => { s.storage.discard(staged).await; return Err(e); }
    };
    let size_bytes = staged.size as i64;
    let (etag, encrypted) = s.storage.commit_mode(staged, &new_object_key, want_enc, asym).await?;

    // The archived row is the PREVIOUS version: its own note, uploader and time.
    if keep_versions {
        sqlx::query(
            r#"INSERT INTO file_versions (id, file_id, version, object_key, size_bytes, etag, uploaded_by, note, encrypted, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"#
        )
        .bind(crate::Id::new()).bind(file.id).bind(file.version)
        .bind(&old_key).bind(file.size_bytes).bind(&file.etag)
        .bind(cur_by.clone().unwrap_or_else(|| file.owner.clone())).bind(&cur_note).bind(file.encrypted).bind(file.modified_at)
        .execute(&s.db).await?;
    }
    let now = Utc::now();
    sqlx::query(
        r#"UPDATE files SET version = $1, object_key = $2, size_bytes = $3, etag = $4, encrypted = $5, modified_at = $6,
                            version_note = $8, version_by = $9 WHERE id = $7"#
    )
    .bind(new_version).bind(&new_object_key).bind(size_bytes).bind(&etag).bind(encrypted).bind(now).bind(file.id)
    .bind(&note).bind(&uploaded_by)
    .execute(&s.db).await?;
    // Folder set not to keep versions: the previous bytes go.
    if !keep_versions && old_key != new_object_key {
        let _ = s.storage.delete(&old_key).await;
    }
    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate',$3,$4,$5,$6,$7,$8,$9,$10)"#
    )
    .bind(crate::Id::new()).bind(&uploaded_by).bind(format!("uploaded v{} of", new_version))
    .bind(&file.name).bind(&file.file_type).bind(file.id)
    .bind(&file.system_id).bind(&file.org_id).bind(now).bind(&user.id)
    .execute(&s.db).await?;
    spawn_p1(s, file.id, file.file_type.clone(), display_name, new_object_key, encrypted, size_bytes as u64);
    let updated: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1")).bind(file.id).fetch_one(&s.db).await?;
    crate::webhooks::emit(&s.db, "file.version", &updated.system_id, crate::webhooks::file_data(&updated)).await;
    Ok(updated)
}

// -----------------------------------------------------------------------------
// Move / rename — TOR 4.15.12
// -----------------------------------------------------------------------------
pub async fn patch_file(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
    Json(p): Json<PatchFile>,
) -> ApiResult<Json<File>> {
    let file: File = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    let my_level = crate::acl::require_file(&s.db, &user.0, id, crate::acl::EDIT).await?;
    if p.name.as_ref().map(|n| n != &file.name).unwrap_or(false)
        && crate::basket::content_locked(&s.db, &file.system_id, &file.status).await? {
        return Err(ApiError::BadRequest("This file's status locks it. Change the status before renaming.".into()));
    }
    if let Some(ref st) = p.status {
        if st != &file.status {
            let need: Option<i16> = sqlx::query_scalar(
                "SELECT required_level FROM basket_status_transitions WHERE system_id = $1 AND from_code = $2 AND to_code = $3",
            ).bind(&file.system_id).bind(&file.status).bind(st).fetch_optional(&s.db).await?;
            match need {
                None => return Err(ApiError::BadRequest(format!("This basket doesn't allow moving from \"{}\" to \"{st}\".", file.status))),
                Some(n) if (my_level as i16) < n => return Err(ApiError::Forbidden),
                _ => {}
            }
        }
    }
    // A status that locks the file also locks its category and fields; only
    // Manage (the reviewer/approver) may still correct them.
    if (p.category_code.is_some() || p.fields.is_some()) && my_level < crate::acl::MANAGE
        && crate::basket::content_locked(&s.db, &file.system_id, &file.status).await? {
        return Err(ApiError::BadRequest("This file's status locks it. Change the status before editing its details.".into()));
    }
    if let Some(ref c) = p.category_code {
        crate::basket::check_category(&s.db, &file.system_id, c.as_deref()).await?;
        sqlx::query("UPDATE files SET category_code = $1 WHERE id = $2").bind(c.as_deref().filter(|v| !v.is_empty())).bind(id).execute(&s.db).await?;
    }
    if let Some(ref fv) = p.fields {
        let mut merged: serde_json::Value = serde_json::from_str(&file.metadata).unwrap_or(serde_json::json!({}));
        if let (Some(m), Some(n)) = (merged.as_object_mut(), fv.as_object()) {
            for (k, v) in n { if v.is_null() { m.remove(k); } else { m.insert(k.clone(), v.clone()); } }
        }
        let checked = crate::basket::validate_fields(&s.db, &file.system_id, &merged, true).await?;
        sqlx::query("UPDATE files SET metadata = $1 WHERE id = $2").bind(checked.to_string()).bind(id).execute(&s.db).await?;
    }

    let new_system: Option<System> = match &p.system_id {
        Some(sid) if sid != &file.system_id => {
            crate::auth::ensure_system_access(&s.db, &user.0, sid).await?;
            Some(
                sqlx::query_as("SELECT * FROM systems WHERE id = $1 AND deleted_at IS NULL")
                    .bind(sid).fetch_optional(&s.db).await?
                    .ok_or_else(|| ApiError::BadRequest("unknown system_id".into()))?
            )
        },
        _ => None,
    };

    // Changing basket or folder is a move: the same checks as POST /api/move —
    // Edit on the destination and the destination's file rules — before any
    // bytes are renamed. A basket change without a folder lands in its root.
    let dest_folder: Option<String> = match &p.folder_id {
        Some(v) => v.clone().filter(|f| !f.is_empty()),
        None if new_system.is_some() => None,
        None => file.folder_id.clone(),
    };
    let dest_system = new_system.as_ref().map(|x| x.id.clone()).unwrap_or_else(|| file.system_id.clone());
    if new_system.is_some() || dest_folder != file.folder_id {
        if new_system.as_ref().is_some_and(|x| x.status == "paused") {
            return Err(ApiError::BadRequest("The destination basket doesn't accept files.".into()));
        }
        crate::acl::require_folder(&s.db, &user.0, &dest_system, dest_folder.as_deref(), crate::acl::EDIT).await
            .map_err(|e| if matches!(e, ApiError::NotFound) { ApiError::Forbidden } else { e })?;
        let check_name = p.name.clone().unwrap_or_else(|| file.name.clone());
        crate::basket::check_policy(&s.db, &dest_system, dest_folder.as_deref(), &check_name, file.size_bytes).await?;
    }

    let new_name = p.name.clone().unwrap_or_else(|| file.name.clone());
    let mut new_object_key = file.object_key.clone();
    let mut new_bucket = file.bucket.clone();

    if let Some(sys) = &new_system {
        new_bucket = sys.bucket.clone();
        let safe = sanitize_filename(&new_name);
        new_object_key = format!("{}/{}/{}-{}", sys.bucket, crate::storage::shard(&file.id.to_string()), file.id, safe);
        // Moving into another bucket counts against that bucket's quota.
        enforce_quota(&s, file.size_bytes, Some(&sys.id), file.org_id.as_deref(), None).await?;
        s.storage.rename(&file.object_key, &new_object_key).await?;
    }
    // A plain rename only changes the display name — the object key keeps the
    // original name (it is an opaque id), so no bytes are rewritten.  The old
    // code re-uploaded the whole file through memory on every rename.

    let tags_json = match p.tags {
        Some(v) => v.to_string(),
        None    => file.tags.clone(),
    };

    let new_file_type = if p.name.is_some() {
        detect_file_type(&new_name)
    } else {
        file.file_type.clone()
    };

    let new_folder_id: Option<String> = dest_folder.clone();

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
    .bind(crate::Id::new())
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
        let v = crate::acl::visibility(&s.db, &user.0, vec_binds.last().unwrap()).await?;
        n += 1; let fln = n; vec_binds.push(v.folders);
        sql.push_str(&format!(" AND folders.id = ANY(${fln})"));
    }
    let _ = n;
    sql.push_str(" ORDER BY name");
    let mut q = sqlx::query_as::<_, Folder>(&sql);
    for b in str_binds { q = q.bind(b); }
    for b in vec_binds { q = q.bind(b); }
    let mut out = q.fetch_all(&s.db).await?;
    crate::basket::fill_effective_encrypted(&s.db, &mut out, s.storage.encryption_enabled()).await?;
    Ok(Json(out))
}

pub async fn create_folder(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(c): Json<CreateFolder>,
) -> ApiResult<Json<Folder>> {
    crate::auth::ensure_system_access(&s.db, &user.0, &c.system_id).await?;
    if let Some(ref pid) = c.parent_id {
        let ps: Option<(String,)> = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL")
            .bind(pid).fetch_optional(&s.db).await?;
        if ps.map(|(x,)| x != c.system_id).unwrap_or(true) {
            return Err(ApiError::BadRequest("parent folder is not in this basket".into()));
        }
    }
    crate::acl::require_folder(&s.db, &user.0, &c.system_id, c.parent_id.as_deref(), crate::acl::EDIT).await?;
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM systems WHERE id = $1 AND deleted_at IS NULL")
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

    let name = c.name.trim().to_string();
    if !crate::basket::valid_folder_name(&name) { return Err(ApiError::BadRequest("Folder name must be 1–200 characters, without / or \\, and not . or ..".into())); }
    if crate::basket::folder_name_taken(&s.db, &c.system_id, c.parent_id.as_deref(), &name, None).await? {
        return Err(ApiError::BadRequest(format!("A folder named \"{name}\" is already here.")));
    }
    // System tables use CUID2 — readable + sortable enough for system rows.
    let id = crate::new_id();
    let owner = c.owner.unwrap_or_else(|| user.0.display_name.clone());
    sqlx::query(
        r#"INSERT INTO folders (id, system_id, org_id, parent_id, name, color, owner, encrypted, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#
    )
    .bind(&id).bind(&c.system_id).bind(&c.org_id).bind(&c.parent_id)
    .bind(&name).bind(&c.color).bind(&owner).bind(c.encrypted.unwrap_or(false)).bind(&user.0.id)
    .execute(&s.db).await?;
    // Options chosen when creating: they become the folder's own policy.
    let mode = match c.encryption.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        None => None,
        Some(m @ ("symmetric" | "asymmetric")) => Some(m.to_string()),
        Some(_) => return Err(ApiError::BadRequest("encryption must be symmetric or asymmetric".into())),
    };
    let enc = if mode.is_some() { Some(true) } else { c.encrypted.filter(|e| *e) };
    if enc.is_some() && !s.storage.has_key() {
        return Err(ApiError::BadRequest("No encryption key is configured on the server, so encrypted folders can't be created.".into()));
    }
    if mode.as_deref() == Some("asymmetric") && !s.storage.has_rsa_key() {
        return Err(ApiError::BadRequest("No RSA key is configured on the server, so asymmetric-encrypted folders can't be created.".into()));
    }
    if enc.is_some() || c.versioning.is_some() {
        sqlx::query("INSERT INTO basket_policies (scope_type, scope_id, encrypt, keep_versions, encrypt_mode) VALUES ('folder', $1, $2, $3, $4)
                     ON CONFLICT (scope_type, scope_id) DO UPDATE SET encrypt = EXCLUDED.encrypt, keep_versions = EXCLUDED.keep_versions, encrypt_mode = EXCLUDED.encrypt_mode")
            .bind(&id).bind(enc).bind(c.versioning).bind(&mode).execute(&s.db).await?;
    }

    let mut f = vec![sqlx::query_as::<_, Folder>("SELECT * FROM folders WHERE id = $1")
        .bind(&id).fetch_one(&s.db).await?];
    crate::basket::fill_effective_encrypted(&s.db, &mut f, s.storage.encryption_enabled()).await?;
    Ok(Json(f.pop().unwrap()))
}

pub async fn delete_folder(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL"
    ).bind(&id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;
    crate::acl::require_folder(&s.db, &user.0, &system_id, Some(&id), crate::acl::EDIT).await?;
    let locked: bool = sqlx::query_scalar(
        r#"WITH RECURSIVE sub(id) AS (SELECT $1::text UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id)
           SELECT EXISTS (SELECT 1 FROM files fi JOIN object_locks l ON l.file_id = fi.id
                           WHERE fi.folder_id IN (SELECT id FROM sub) AND fi.deleted_at IS NULL
                             AND (l.legal_hold OR l.retain_until > now()))"#, // same rule as objectlock::LOCKED_NOW, over a folder tree
    ).bind(&id).fetch_one(&s.db).await?;
    if locked { return Err(ApiError::Denied("This folder holds locked files, so it can't be deleted.".into())); }
    // The folder, its sub-folders and the files in them all go to Trash with the
    // same time, so the files show in Trash, drop out of search/Recent, and
    // restoring one of them brings its folders back (TOR 4.15.8).
    let mut tx = s.db.begin().await?;
    let r = sqlx::query(
        r#"WITH RECURSIVE sub(id) AS (SELECT $1::text UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id WHERE f.deleted_at IS NULL)
           UPDATE folders SET deleted_at = now() WHERE id IN (SELECT id FROM sub) AND deleted_at IS NULL"#)
        .bind(&id).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    let files: Vec<(Id, String, String)> = sqlx::query_as(
        r#"WITH RECURSIVE sub(id) AS (SELECT $1::text UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id)
           UPDATE files SET deleted_at = now(), modified_at = now()
            WHERE folder_id IN (SELECT id FROM sub) AND deleted_at IS NULL
           RETURNING id, name, file_type"#)
        .bind(&id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for (fid, name, ft) in &files {
        crate::basket::log_activity(&s.db, &user.0, "sent to trash", name, ft, Some(*fid), &system_id).await;
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
    Path(file_id): Path<Id>,
    Json(c): Json<CreateShareLink>,
) -> ApiResult<Json<ShareResponse>> {
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
    crate::acl::require_file(&s.db, &user.0, file.id, crate::acl::EDIT).await?;
    let pol = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?;
    if pol.public_links == Some(false) {
        return Err(ApiError::BadRequest("Public links are turned off for this basket.".into()));
    }

    let token = random_token();
    // Share links are public credentials — a TTL is mandatory. We default to
    // 7 days when the caller omits `expires_in_days` and cap to 90 even when
    // the caller asks for more, so a stale email can't unlock a file forever.
    const DEFAULT_TTL_DAYS: i64 = 7;
    const MAX_TTL_DAYS:     i64 = 90;
    let ttl_days = c.expires_in_days.unwrap_or(DEFAULT_TTL_DAYS).clamp(1, MAX_TTL_DAYS);
    let expires_at = Some(Utc::now() + Duration::days(ttl_days));
    let id = crate::Id::new();
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
    .bind(crate::Id::new())
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
    let row: Option<(Id, Option<chrono::DateTime<Utc>>)> = sqlx::query_as(
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
    headers_in: HeaderMap,
) -> ApiResult<axum::response::Response> {
    let file = share_target(&s, &token).await?;
    // Content-Disposition now goes through the RFC 5987 helper — the old
    // `format!("attachment; filename=\"{}\"")` panicked on Thai file names.
    crate::serve::stream_object(&s, crate::serve::ServeOpts {
        key: &file.object_key,
        encrypted: file.encrypted,
        name: &file.name,
        etag: file.etag.as_deref(),
        disposition: "attachment",
        cache_control: "private, no-store",
    }, &headers_in).await
}

// -----------------------------------------------------------------------------
// Reports — TOR 4.15.15
// -----------------------------------------------------------------------------
pub async fn report_by_category(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<ReportCatQuery>,
) -> ApiResult<Json<Vec<CategoryReportRow>>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    // by = category (basket categories, the TOR's "document categories") | format | basket | type (old behaviour)
    let expr = match q.by.as_deref().unwrap_or("category") {
        "category" => "COALESCE((SELECT c.name FROM basket_categories c WHERE c.system_id = f.system_id AND c.code = f.category_code), 'No category')",
        "format" => "fh_format(f.name)",
        "basket" => "(SELECT sy.name FROM systems sy WHERE sy.id = f.system_id)",
        "type" => "f.file_type",
        _ => return Err(ApiError::BadRequest("by must be category, format, basket or type".into())),
    };
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;
    // WHERE is built from the filters actually given (no NULL-guard flags in SQL).
    let mut w = String::from("f.deleted_at IS NULL");
    let mut n = 0;
    if q.basket.is_some() { n += 1; w.push_str(&format!(" AND f.system_id = ${n}")); }
    if scope.is_some() { n += 1; w.push_str(&format!(" AND f.system_id = ANY(${n})")); }
    let sql = format!(
        "SELECT {expr} AS category, COUNT(*)::bigint AS file_count, COALESCE(SUM(f.size_bytes), 0)::bigint AS size_bytes \
           FROM files f WHERE {w} GROUP BY 1 ORDER BY size_bytes DESC");
    let mut qy = sqlx::query_as(&sql);
    if let Some(b) = &q.basket { qy = qy.bind(b); }
    if let Some(sc) = &scope { qy = qy.bind(sc); }
    Ok(Json(qy.fetch_all(&s.db).await?))
}

pub async fn report_by_time(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<ReportTimeQuery>,
) -> ApiResult<Json<Vec<TimeReportRow>>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;
    let mut w = String::from("deleted_at IS NULL");
    let mut n = 1;
    if q.basket.is_some() { n += 1; w.push_str(&format!(" AND system_id = ${n}")); }
    if scope.is_some() { n += 1; w.push_str(&format!(" AND system_id = ANY(${n})")); }
    let bucket = q.bucket.clone().unwrap_or_else(|| "day".into());
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
            WHERE {w}
            GROUP BY 1
            ORDER BY 1 DESC
            LIMIT $1"#
    );
    let mut qy = sqlx::query_as(&sql).bind(limit);
    if let Some(b) = &q.basket { qy = qy.bind(b); }
    if let Some(sc) = &scope { qy = qy.bind(sc); }
    Ok(Json(qy.fetch_all(&s.db).await?))
}

// =============================================================================
// Workspace config — Q1: PATCH /api/workspace
// =============================================================================
/// Public branding for pages shown before sign-in (login, access denied) and
/// for the app shell.  Only presentation keys — never quotas or policies.
pub async fn branding(State(s): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    const KEYS: &[&str] = &["workspace_display", "org_name", "brand_accent", "login_email_placeholder",
                            "portal_url", "portal_label", "access_help", "self_signup", "brand_name"];
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM workspace_config WHERE key = ANY($1)")
        .bind(KEYS.iter().map(|k| k.to_string()).collect::<Vec<_>>())
        .fetch_all(&s.db).await?;
    let mut m = serde_json::Map::new();
    for k in KEYS { m.insert((*k).into(), serde_json::Value::String(String::new())); }
    for (k, v) in rows { m.insert(k, serde_json::Value::String(v)); }
    // Accept only a plain hex colour so the value can go straight into CSS.
    if let Some(serde_json::Value::String(c)) = m.get("brand_accent") {
        let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
        if !ok { m.insert("brand_accent".into(), serde_json::Value::String(String::new())); }
    }
    // Where browsers send file bytes (UPLOAD_BASE_URL, e.g. https://upload.example.go.th/filehub);
    // empty = this host. Set by the deployment, not by admins.
    // Which deployment this is (ENV_LABEL, e.g. UAT), shown on the sign-in page. Empty = none.
    m.insert("env_label".into(), serde_json::Value::String(
        std::env::var("ENV_LABEL").unwrap_or_default().trim().chars().take(24).collect()));
    m.insert("upload_base".into(), serde_json::Value::String(
        std::env::var("UPLOAD_BASE_URL").unwrap_or_default().trim().trim_end_matches('/').to_string()));
    // One URL that carries every chunked upload (UPLOAD_PATH, e.g. /FileService/upload,
    // for an edge that lets file bytes through on that path only); empty = the usual paths.
    m.insert("upload_path".into(), serde_json::Value::String(
        std::env::var("UPLOAD_PATH").unwrap_or_default().trim().to_string()));
    Ok(Json(serde_json::Value::Object(m)))
}

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
    #[sqlx(default)]
    pub default_org_id: Option<String>,
    #[sqlx(default)]
    pub source: Option<String>,
    #[sqlx(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
}

pub async fn list_members(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<MemberRow>>> {
    // Everyone's name and email: admins only (sharing uses /api/principals).
    crate::auth::require_role(&user.0, &["admin"])?;
    let rows: Vec<MemberRow> = sqlx::query_as(
        // One pass over sessions and one over files (not two lookups per person).
        r#"WITH seen AS (SELECT user_id, MAX(last_seen_at) AS last_seen_at FROM sessions GROUP BY user_id),
                used AS (SELECT created_by, SUM(size_bytes) AS used_bytes FROM files WHERE deleted_at IS NULL GROUP BY created_by)
           SELECT u.id, u.email, u.display_name, u.avatar_tone, u.role, u.status,
                  u.quota_bytes, u.created_at, u.default_org_id, u.source,
                  seen.last_seen_at, COALESCE(used.used_bytes, 0)::bigint AS used_bytes
             FROM users u
             LEFT JOIN seen ON seen.user_id = u.id
             LEFT JOIN used ON used.created_by = u.id
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
    let id = crate::new_id();
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
    /// The person's agency ("" clears it) — agency grants (`org:<id>`) follow it.
    pub default_org_id: Option<String>,
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

    if let Some(o) = body.default_org_id.as_deref().filter(|v| !v.is_empty()) {
        let ok: Option<String> = sqlx::query_scalar("SELECT id FROM orgs WHERE id = $1").bind(o).fetch_optional(&s.db).await?;
        if ok.is_none() { return Err(ApiError::BadRequest("unknown agency".into())); }
    }
    if let Some(o) = body.default_org_id.as_deref() {
        sqlx::query("UPDATE users SET default_org_id = NULLIF($2, '') WHERE id = $1").bind(&id).bind(o).execute(&s.db).await?;
    }
    if body.status.as_deref() == Some("disabled") {
        // Turned off = signed out everywhere now, not at the next login.
        sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(&id).execute(&s.db).await?;
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
                  u.quota_bytes, u.created_at, u.default_org_id, u.source,
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
    // Totals come from the shared 30 s cache (also used by the upload quota
    // check), so opening a page never re-adds the whole workspace.
    let ws_used = cached_sum(&s.db, "ws", "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE deleted_at IS NULL", None).await?;
    scopes.push(QuotaScope { scope: "workspace", id: None, limit_bytes: ws_cap, used_bytes: ws_used });

    // Only baskets the caller can reach; other agencies' totals are admin-only.
    if let Some(sid) = q.system_id.as_deref() { crate::auth::ensure_system_access(&s.db, &user.0, sid).await?; }
    if q.org_id.is_some() && user.0.role != "admin" { return Err(ApiError::Forbidden); }
    if let Some(sid) = q.system_id.as_deref() {
        let cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM systems WHERE id = $1")
            .bind(sid).fetch_optional(&s.db).await?.unwrap_or(0);
        let used = cached_sum(&s.db, "sys", "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE system_id = $1 AND deleted_at IS NULL", Some(sid)).await?;
        scopes.push(QuotaScope { scope: "system", id: Some(sid.to_string()), limit_bytes: cap, used_bytes: used });
    }
    if let Some(oid) = q.org_id.as_deref() {
        let cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM orgs WHERE id = $1")
            .bind(oid).fetch_optional(&s.db).await?.unwrap_or(0);
        let used = cached_sum(&s.db, "org", "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE org_id = $1 AND deleted_at IS NULL", Some(oid)).await?;
        scopes.push(QuotaScope { scope: "org", id: Some(oid.to_string()), limit_bytes: cap, used_bytes: used });
    }
    let user_cap: i64 = sqlx::query_scalar("SELECT quota_bytes FROM users WHERE id = $1")
        .bind(&user.0.id).fetch_optional(&s.db).await?.unwrap_or(0);
    let user_used = cached_sum(&s.db, "user", "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE created_by = $1 AND deleted_at IS NULL", Some(&user.0.id)).await?;
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
    let new_id = crate::new_id();
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
    pub id: Id,
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
    let id = crate::Id::new();
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
    .bind(crate::Id::new()).bind(&user.0.display_name).bind(&user.0.avatar_tone)
    .bind(name).bind(&user.0.id)
    .execute(&s.db).await;

    Ok(Json(CreatedApiKey { key: plaintext, meta }))
}

pub async fn revoke_api_key(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
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
        assert!(parse_uuid("00000000-0000-4000-8000-000000000001").is_err());
        assert!(parse_uuid(&crate::new_id()).is_ok());
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

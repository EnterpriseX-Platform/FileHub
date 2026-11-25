//! Chunked, resumable upload — TUS 1.0.0 (https://tus.io/protocols/resumable-upload).
//!
//! One path for every upload, small or large:
//!
//!   POST   /fh/api/uploads                    start (signed in)
//!   HEAD   /fh/api/uploads/:id                how much the server has
//!   PATCH  /fh/api/uploads/:id                send the next chunk
//!   DELETE /fh/api/uploads/:id                cancel
//!
//!   POST   /fh/api/upload/:token/chunks       same, through a public upload link
//!   HEAD/PATCH/DELETE /fh/api/upload/:token/chunks/:id
//!
//! `Upload-Metadata` (base64 values) says what the upload is for:
//!   purpose=file     (default) a new file — system_id, folder_id, category_code,
//!                    fields (JSON), tags, project, status, org_id, filename
//!   purpose=version  a new version — file_id, note, filename
//!   link uploads     submitter_name, submitter_email, category_code, fields
//!
//! Every rule a regular upload has (access, allowed types, size, quota,
//! category, required fields, locked status) is checked when the upload
//! starts — before any byte is sent — and again when the last chunk arrives,
//! through the same code as a regular upload (`persist_upload_for`,
//! `commit_version`, `uploadlink::accept`).  The last PATCH answers with
//! `Upload-File-Id`; HEAD on a finished upload repeats it.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use tokio::fs;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use crate::Id;

use crate::auth::{AuthUser, MaybeAuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const TUS_VERSION: &str = "1.0.0";

/// Largest single chunk (`UPLOAD_CHUNK_MAX_MB`, default 64). Chunks sit in
/// memory while written, so this — not the file size — bounds RAM per request.
pub fn chunk_max_bytes() -> usize {
    std::env::var("UPLOAD_CHUNK_MAX_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(64) * 1024 * 1024
}

fn tus_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(HeaderName::from_static("tus-resumable"), HeaderValue::from_static(TUS_VERSION));
    h.insert(HeaderName::from_static("tus-version"),   HeaderValue::from_static(TUS_VERSION));
    h.insert(HeaderName::from_static("tus-extension"), HeaderValue::from_static("creation,termination"));
    h.insert(HeaderName::from_static("cache-control"), HeaderValue::from_static("no-store"));
    h
}

fn num(n: i64) -> HeaderValue { HeaderValue::from_str(&n.to_string()).unwrap() }

/// Where partial uploads live until they are complete.
/// Where partial uploads live until complete — shared by all pods (`SPOOL_DIR`).
fn tus_root() -> std::path::PathBuf {
    crate::storage::spool_root().join(".tus")
}

/// `Upload-Metadata`: comma-separated `key base64value` pairs.
fn parse_upload_metadata(raw: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for entry in raw.split(',') {
        let mut it = entry.trim().splitn(2, ' ');
        let key = match it.next() { Some(k) if !k.is_empty() => k, _ => continue };
        let val = match it.next() {
            Some(v) => B64.decode(v.trim()).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_default(),
            None => String::new(),
        };
        out.insert(key.to_string(), val);
    }
    out
}

fn bad(m: impl Into<String>) -> ApiError { ApiError::BadRequest(m.into()) }

/// Who is talking to a session: a signed-in person, or someone holding an
/// upload link (maybe also signed in).
enum Caller<'a> { User(&'a User), Link { token: &'a str, user: Option<&'a User> } }

// ─────────────────────────── start ───────────────────────────

pub async fn create_session(State(s): State<Arc<AppState>>, user: AuthUser, headers: HeaderMap) -> ApiResult<Response> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    crate::auth::upload_rate_limit(&s.db, &user.0.id).await?;
    create(&s, Caller::User(&user.0), &headers).await
}

pub async fn link_create(State(s): State<Arc<AppState>>, user: MaybeAuthUser, Path(token): Path<String>, headers: HeaderMap) -> ApiResult<Response> {
    crate::auth::upload_rate_limit_keyed(&s.db, &format!("ul:{token}"), 120).await?;
    create(&s, Caller::Link { token: &token, user: user.0.as_ref() }, &headers).await
}

async fn create(s: &AppState, caller: Caller<'_>, headers: &HeaderMap) -> ApiResult<Response> {
    let length: i64 = headers.get("upload-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok())
        .ok_or_else(|| bad("Upload-Length required"))?;
    if length < 0 { return Err(bad("Upload-Length must be non-negative")); }
    let meta = parse_upload_metadata(headers.get("upload-metadata").and_then(|v| v.to_str().ok()).unwrap_or(""));
    // Empty values mean "not given" (clients send every key).
    let pick = |k: &str| meta.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let filename = pick("filename").or_else(|| pick("name")).unwrap_or_else(|| "upload.bin".into());
    if filename.len() > 255 { return Err(bad("File name is too long.")); }
    let fields_json: serde_json::Value = match pick("fields") {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| bad("fields must be a JSON object"))?,
        None => serde_json::Value::Null,
    };

    let id = crate::Id::new();
    let temp_key = format!("{id}.part");
    let mut row = Row { purpose: "file".into(), ..Default::default() };
    // "Encrypt this file" — allowed only where the basket/folder policy lets people choose.
    let want_file_enc = matches!(pick("encrypt").as_deref(), Some("1" | "true" | "yes" | "on"));

    match caller {
        Caller::User(u) => {
            row.user_id = Some(u.id.clone());
            if pick("purpose").as_deref() == Some("version") {
                let fid = pick("file_id").and_then(|v| crate::Id::parse_str(&v).ok()).ok_or_else(|| bad("file_id required for a new version"))?;
                let file = crate::handlers::check_version_target(s, u, fid).await?;
                crate::handlers::enforce_quota(s, length, Some(&file.system_id), file.org_id.as_deref(), Some(&u.id)).await?;
                crate::basket::check_upload_policy(&s.db, &file.system_id, file.folder_id.as_deref(), &filename, length).await?;
                if want_file_enc {
                    let pol = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?;
                    crate::handlers::check_file_choice(s, &pol)?;
                    row.encrypt_file = true;
                }
                row.purpose = "version".into();
                row.target_file_id = Some(fid);
                row.system_id = Some(file.system_id);
                row.note = pick("note");
            } else {
                let sys = pick("system_id").ok_or_else(|| bad("system_id required in upload metadata"))?;
                crate::auth::ensure_system_access(&s.db, u, &sys).await?;
                let folder = pick("folder_id");
                crate::acl::require_folder(&s.db, u, &sys, folder.as_deref(), crate::acl::EDIT).await
                    .map_err(|e| if matches!(e, ApiError::NotFound) { ApiError::Forbidden } else { e })?;
                let pol = crate::basket::check_upload_policy(&s.db, &sys, folder.as_deref(), &filename, length).await?;
                // Same name in this folder = a new version of that file: refuse now
                // (before any bytes are sent) when its status locks the content.
                if let Some(same) = crate::handlers::same_name_file(s, &sys, folder.as_deref(), &filename).await? {
                    crate::handlers::check_version_target(s, u, same).await?;
                }
                if want_file_enc {
                    crate::handlers::check_file_choice(s, &pol)?;
                    row.encrypt_file = true;
                }
                if pol.encrypt == Some(true) && !s.storage.has_key() {
                    return Err(bad("This folder requires encryption, but no encryption key is set up on the server (STORAGE_ENC_KEY)."));
                }
                crate::handlers::enforce_quota(s, length, Some(&sys), pick("org_id").as_deref(), Some(&u.id)).await?;
                crate::basket::check_category(&s.db, &sys, pick("category_code").as_deref()).await?;
                if !fields_json.is_null() {
                    crate::basket::validate_fields(&s.db, &sys, &fields_json, u.role != "admin").await?;
                }
                if let Some(st) = pick("status") {
                    let ok: Option<(String,)> = sqlx::query_as("SELECT code FROM basket_statuses WHERE system_id = $1 AND code = $2")
                        .bind(&sys).bind(&st).fetch_optional(&s.db).await?;
                    if ok.is_none() { return Err(bad(format!("Status \"{st}\" does not exist in this basket."))); }
                }
                row.system_id = Some(sys);
                row.folder_id = folder;
                row.org_id = pick("org_id");
                row.project = pick("project");
                row.status = pick("status");
                row.owner = pick("owner");
                row.tags = pick("tags");
            }
        }
        Caller::Link { token, user } => {
            let l = crate::uploadlink::live(s, token).await?;
            if l.require_login && user.is_none() { return Err(ApiError::Unauthorized); }
            let name = user.map(|u| u.display_name.clone()).or_else(|| pick("submitter_name"));
            let email = user.map(|u| u.email.clone()).or_else(|| pick("submitter_email"));
            crate::uploadlink::check_submission(s, &l, name.clone(), email.clone(), pick("category_code"), fields_json.clone(), 1).await?;
            crate::basket::check_upload_policy(&s.db, &l.sys, l.fld.as_deref(), &filename, length).await?;
            crate::handlers::enforce_quota(s, length, Some(&l.sys), None, Some(&l.creator.id)).await?;
            row.purpose = "link".into();
            row.link_token = Some(token.to_string());
            row.user_id = user.map(|u| u.id.clone());
            row.system_id = Some(l.sys);
            row.folder_id = l.fld;
            row.submitter_name = name;
            row.submitter_email = email;
        }
    }
    row.category_code = pick("category_code");
    row.fields = if fields_json.is_null() { None } else { Some(fields_json.to_string()) };

    let dir = tus_root();
    fs::create_dir_all(&dir).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    fs::File::create(dir.join(&temp_key)).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    sqlx::query(
        r#"INSERT INTO tus_uploads
            (id, user_id, upload_length, upload_offset, filename, content_type, system_id, org_id, folder_id,
             project, status_meta, owner_meta, tags_meta, temp_key, purpose, target_file_id, link_token,
             category_code, fields_meta, note_meta, submitter_name, submitter_email, encrypt_file)
           VALUES ($1,$2,$3,0,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)"#,
    )
    .bind(id).bind(&row.user_id).bind(length).bind(&filename).bind(pick("content_type").or_else(|| pick("filetype")))
    .bind(&row.system_id).bind(&row.org_id).bind(&row.folder_id).bind(&row.project).bind(&row.status)
    .bind(&row.owner).bind(&row.tags).bind(&temp_key).bind(&row.purpose).bind(row.target_file_id)
    .bind(&row.link_token).bind(&row.category_code).bind(&row.fields).bind(&row.note)
    .bind(&row.submitter_name).bind(&row.submitter_email).bind(row.encrypt_file)
    .execute(&s.db).await?;

    // An upload link's file cap counts sessions still in flight too. Counted
    // after this one is recorded, so parallel sessions can't all slip past it.
    if let Some(token) = &row.link_token {
        let over: bool = sqlx::query_scalar(
            "SELECT COALESCE(l.max_files IS NOT NULL AND l.upload_count + (SELECT COUNT(*) FROM tus_uploads t WHERE t.link_token = $1 AND t.completed_at IS NULL) > l.max_files, false)
               FROM upload_links l WHERE l.token = $1",
        ).bind(token).fetch_optional(&s.db).await?.unwrap_or(false);
        if over {
            sqlx::query("DELETE FROM tus_uploads WHERE id = $1").bind(id).execute(&s.db).await?;
            let _ = fs::remove_file(dir.join(&temp_key)).await;
            return Err(bad("This link has taken all the files it accepts."));
        }
    }

    // Relative Location resolves right both through the app proxy
    // (/filehub/api/…) and straight to the backend (/fh/api/…).
    let mut h = tus_headers();
    h.insert(HeaderName::from_static("location"), HeaderValue::from_str(&format!("{}/{id}", if row.link_token.is_some() { "chunks" } else { "uploads" })).unwrap());
    h.insert(HeaderName::from_static("upload-offset"), num(0));
    if length == 0 {
        // Nothing to send: finish now.
        let fid = finish(s, id).await?;
        if let Some(f) = fid { h.insert(HeaderName::from_static("upload-file-id"), HeaderValue::from_str(&f.to_string()).unwrap()); }
    }
    Ok((StatusCode::CREATED, h).into_response())
}

#[derive(Default)]
struct Row {
    purpose: String, user_id: Option<String>, system_id: Option<String>, folder_id: Option<String>, org_id: Option<String>,
    project: Option<String>, status: Option<String>, owner: Option<String>, tags: Option<String>,
    target_file_id: Option<Id>, link_token: Option<String>, category_code: Option<String>, fields: Option<String>,
    note: Option<String>, submitter_name: Option<String>, submitter_email: Option<String>,
    encrypt_file: bool,
}

/// The session, if this caller may touch it.
async fn session(s: &AppState, id: Id, caller: &Caller<'_>) -> ApiResult<(i64, i64, String, Option<Id>, bool)> {
    let row: Option<(Option<String>, Option<String>, i64, i64, String, Option<Id>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT user_id, link_token, upload_length, upload_offset, temp_key, file_id, completed_at FROM tus_uploads WHERE id = $1",
    ).bind(id).fetch_optional(&s.db).await?;
    let (owner, link, length, offset, temp, file_id, done) = row.ok_or(ApiError::NotFound)?;
    let ok = match caller {
        Caller::User(u) => link.is_none() && owner.as_deref() == Some(u.id.as_str()),
        Caller::Link { token, user } => link.as_deref() == Some(*token) && (owner.is_none() || owner.as_deref() == user.map(|u| u.id.as_str())),
    };
    if !ok { return Err(ApiError::NotFound); }
    Ok((length, offset, temp, file_id, done.is_some()))
}

// ─────────────────────────── progress ───────────────────────────

pub async fn head_session(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<Response> {
    head(&s, id, Caller::User(&user.0)).await
}
pub async fn link_head(State(s): State<Arc<AppState>>, user: MaybeAuthUser, Path((token, id)): Path<(String, Id)>) -> ApiResult<Response> {
    head(&s, id, Caller::Link { token: &token, user: user.0.as_ref() }).await
}
async fn head(s: &AppState, id: Id, caller: Caller<'_>) -> ApiResult<Response> {
    let (length, offset, _, file_id, _) = session(s, id, &caller).await?;
    let mut h = tus_headers();
    h.insert(HeaderName::from_static("upload-offset"), num(offset));
    h.insert(HeaderName::from_static("upload-length"), num(length));
    if let Some(f) = file_id { h.insert(HeaderName::from_static("upload-file-id"), HeaderValue::from_str(&f.to_string()).unwrap()); }
    Ok((StatusCode::OK, h).into_response())
}

// ─────────────────────────── send a chunk ───────────────────────────

pub async fn append_chunk(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    patch(&s, id, Caller::User(&user.0), &headers, body).await
}
pub async fn link_patch(State(s): State<Arc<AppState>>, user: MaybeAuthUser, Path((token, id)): Path<(String, Id)>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    patch(&s, id, Caller::Link { token: &token, user: user.0.as_ref() }, &headers, body).await
}

async fn patch(s: &AppState, id: Id, caller: Caller<'_>, headers: &HeaderMap, body: Bytes) -> ApiResult<Response> {
    let (length, offset, temp, file_id, done) = session(s, id, &caller).await?;
    let client_offset: i64 = headers.get("upload-offset").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok())
        .ok_or_else(|| bad("Upload-Offset required"))?;
    let ct = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
    if ct != "application/offset+octet-stream" { return Err(bad("Content-Type must be application/offset+octet-stream")); }

    let mut h = tus_headers();
    if done {
        // Already finished (e.g. the reply to the last chunk was lost): say so again.
        h.insert(HeaderName::from_static("upload-offset"), num(offset));
        if let Some(f) = file_id { h.insert(HeaderName::from_static("upload-file-id"), HeaderValue::from_str(&f.to_string()).unwrap()); }
        return Ok((StatusCode::NO_CONTENT, h).into_response());
    }
    if client_offset != offset {
        return Err(ApiError::Conflict(format!("Upload-Offset mismatch: server has {offset}, client sent {client_offset}")));
    }
    let n = body.len() as i64;
    if offset + n > length { return Err(bad("chunk exceeds Upload-Length")); }

    if n > 0 {
        // Write at the offset (not append) so a retried chunk can't double up.
        let path = tus_root().join(&temp);
        let mut f = fs::OpenOptions::new().write(true).open(&path).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
        f.seek(std::io::SeekFrom::Start(offset as u64)).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
        f.write_all(&body).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
        f.flush().await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
        // Two requests racing for the same offset: only one moves it forward.
        let moved = sqlx::query("UPDATE tus_uploads SET upload_offset = $1 WHERE id = $2 AND upload_offset = $3")
            .bind(offset + n).bind(id).bind(offset).execute(&s.db).await?.rows_affected();
        if moved == 0 { return Err(ApiError::Conflict("Another request already sent this chunk.".into())); }
    }
    let new_offset = offset + n;
    h.insert(HeaderName::from_static("upload-offset"), num(new_offset));
    if new_offset == length {
        if let Some(f) = finish(s, id).await? {
            h.insert(HeaderName::from_static("upload-file-id"), HeaderValue::from_str(&f.to_string()).unwrap());
        }
    }
    Ok((StatusCode::NO_CONTENT, h).into_response())
}

// ─────────────────────────── finish ───────────────────────────

/// All bytes are in: hand them to the same code a regular upload uses.
/// A refusal (type, size, fields, access changed meanwhile) ends the session
/// and frees the bytes; a server error leaves it so the last chunk can be retried.
async fn finish(s: &AppState, id: Id) -> ApiResult<Option<Id>> {
    // Claim it, so two final chunks can't both create a file.
    let claimed = sqlx::query("UPDATE tus_uploads SET completed_at = now() WHERE id = $1 AND completed_at IS NULL")
        .bind(id).execute(&s.db).await?.rows_affected();
    if claimed == 0 {
        return Ok(sqlx::query_scalar("SELECT file_id FROM tus_uploads WHERE id = $1").bind(id).fetch_one(&s.db).await?);
    }
    let result = finish_inner(s, id).await;
    let temp: Option<String> = sqlx::query_scalar("SELECT temp_key FROM tus_uploads WHERE id = $1").bind(id).fetch_optional(&s.db).await?;
    match result {
        Ok(fid) => {
            sqlx::query("UPDATE tus_uploads SET file_id = $1 WHERE id = $2").bind(fid).bind(id).execute(&s.db).await?;
            if let Some(t) = temp { let _ = fs::remove_file(tus_root().join(t)).await; }
            Ok(fid)
        }
        Err(e @ (ApiError::BadRequest(_) | ApiError::PayloadTooLarge(_) | ApiError::Forbidden | ApiError::Denied(_) | ApiError::NotFound
                 | ApiError::Unauthorized | ApiError::ContentLocked(_))) => {
            sqlx::query("DELETE FROM tus_uploads WHERE id = $1").bind(id).execute(&s.db).await?;
            if let Some(t) = temp { let _ = fs::remove_file(tus_root().join(t)).await; }
            Err(e)
        }
        Err(e) => {
            sqlx::query("UPDATE tus_uploads SET completed_at = NULL WHERE id = $1").bind(id).execute(&s.db).await?;
            Err(e)
        }
    }
}

async fn finish_inner(s: &AppState, id: Id) -> ApiResult<Option<Id>> {
    let r: Pending = sqlx::query_as("SELECT * FROM tus_uploads WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    let Pending { user_id, filename, content_type, system_id, org_id, folder_id, project, status_meta: status, owner_meta: owner,
         temp_key: temp, purpose, target_file_id: target, link_token: link, category_code: cat, fields_meta: fields,
         note_meta: note, submitter_name: sub_name, submitter_email: sub_email, tags_meta, encrypt_file } = r;
    let tags = tags_meta;

    let user: Option<User> = match &user_id {
        Some(u) => sqlx::query_as("SELECT * FROM users WHERE id = $1 AND status = 'active'").bind(u).fetch_optional(&s.db).await?,
        None => None,
    };
    if user_id.is_some() && user.is_none() { return Err(ApiError::Unauthorized); }

    // Re-stage through the storage layer (encrypts as it streams; O(chunk) memory).
    let src = fs::File::open(tus_root().join(&temp)).await.map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    let staged = s.storage.stage_stream(tokio_util::io::ReaderStream::with_capacity(src, 256 * 1024)).await.map_err(ApiError::Other)?;

    match purpose.as_str() {
        "version" => {
            let u = user.ok_or(ApiError::Unauthorized)?;
            let fid = target.ok_or_else(|| bad("file_id missing"))?;
            let f = crate::handlers::commit_version_enc(s, &u, fid, staged, filename, None, note, encrypt_file).await?;
            Ok(Some(f.id))
        }
        "link" => {
            let token = link.ok_or(ApiError::NotFound)?;
            let l = match crate::uploadlink::live(s, &token).await { Ok(l) => l, Err(e) => { s.storage.discard(staged).await; return Err(e); } };
            let fv: serde_json::Value = fields.as_deref().and_then(|f| serde_json::from_str(f).ok()).unwrap_or(serde_json::Value::Null);
            let sub = match crate::uploadlink::check_submission(s, &l, sub_name, sub_email, cat, fv, 1).await {
                Ok(v) => v, Err(e) => { s.storage.discard(staged).await; return Err(e); }
            };
            let name = filename.unwrap_or_else(|| "upload.bin".into());
            crate::uploadlink::accept(s, &token, &l, &sub, vec![(name, content_type, staged)]).await?;
            // People sending through a link don't get a file id: they can't open it.
            Ok(None)
        }
        _ => {
            let u = user.ok_or(ApiError::Unauthorized)?;
            let up = crate::handlers::UploadFields {
                name: filename, system_id, org_id, folder_id, project, status, owner, tags,
                category_code: cat, fields, content_type, staged: Some(staged), encrypt_file, ..Default::default()
            };
            let f = crate::handlers::persist_or_version(s, &u, up).await?;
            Ok(Some(f.id))
        }
    }
}

#[derive(sqlx::FromRow)]
struct Pending {
    user_id: Option<String>, filename: Option<String>, content_type: Option<String>, system_id: Option<String>,
    org_id: Option<String>, folder_id: Option<String>, project: Option<String>, status_meta: Option<String>,
    owner_meta: Option<String>, tags_meta: Option<String>, temp_key: String, purpose: String, target_file_id: Option<Id>,
    link_token: Option<String>, category_code: Option<String>, fields_meta: Option<String>, note_meta: Option<String>,
    submitter_name: Option<String>, submitter_email: Option<String>,
    #[sqlx(default)]
    encrypt_file: bool,
}

// ─────────────────────────── cancel ───────────────────────────

pub async fn terminate_session(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<Response> {
    terminate(&s, id, Caller::User(&user.0)).await
}
pub async fn link_terminate(State(s): State<Arc<AppState>>, user: MaybeAuthUser, Path((token, id)): Path<(String, Id)>) -> ApiResult<Response> {
    terminate(&s, id, Caller::Link { token: &token, user: user.0.as_ref() }).await
}
async fn terminate(s: &AppState, id: Id, caller: Caller<'_>) -> ApiResult<Response> {
    let (_, _, temp, _, done) = session(s, id, &caller).await?;
    if done { return Err(ApiError::NotFound); }
    sqlx::query("DELETE FROM tus_uploads WHERE id = $1 AND completed_at IS NULL").bind(id).execute(&s.db).await?;
    let _ = fs::remove_file(tus_root().join(&temp)).await;
    Ok((StatusCode::NO_CONTENT, tus_headers()).into_response())
}

// ─────────────────────────── one-path transport ───────────────────────────
//
// The same protocol carried on a single URL (`/FileService/upload`), for
// deployments whose edge only lets file bytes through on that path. Every
// request is a POST: `X-HTTP-Method-Override` says HEAD/PATCH/DELETE, the
// session goes in `Upload-Id` (not in the path), and a chunk is either the raw
// body (`application/offset+octet-stream`) or the file part of an ordinary
// `multipart/form-data` body, its bytes as they are. `Upload-Link: <token>`
// makes it an upload-link session. Same checks and the same finish as above.

fn header<'h>(h: &'h HeaderMap, k: &str) -> Option<&'h str> {
    h.get(k).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty())
}

/// Entry for `/FileService/upload` requests that carry `Tus-Resumable`.
pub async fn one_path(s: Arc<AppState>, req: axum::extract::Request) -> ApiResult<Response> {
    use axum::extract::{FromRequest, FromRequestParts};
    let (mut parts, body) = req.into_parts();
    let link = header(&parts.headers, "upload-link").map(str::to_string);
    let user: Option<User> = if link.is_some() {
        MaybeAuthUser::from_request_parts(&mut parts, &s).await?.0
    } else {
        Some(AuthUser::from_request_parts(&mut parts, &s).await?.0)
    };
    let headers = parts.headers.clone();
    let caller = match (&link, &user) {
        (Some(t), u) => Caller::Link { token: t.as_str(), user: u.as_ref() },
        (None, Some(u)) => Caller::User(u),
        (None, None) => return Err(ApiError::Unauthorized),
    };
    let method = if parts.method == axum::http::Method::POST {
        header(&headers, "x-http-method-override").unwrap_or("POST").to_ascii_uppercase()
    } else { parts.method.as_str().to_string() };
    let id = || header(&headers, "upload-id").and_then(|v| Id::parse_str(v).ok()).ok_or_else(|| bad("Upload-Id required"));
    match method.as_str() {
        "POST" => {
            match &caller {
                Caller::User(u) => {
                    crate::auth::require_role(u, &["admin", "editor"])?;
                    crate::auth::upload_rate_limit(&s.db, &u.id).await?;
                }
                Caller::Link { token, .. } => crate::auth::upload_rate_limit_keyed(&s.db, &format!("ul:{token}"), 120).await?,
            }
            let mut r = create(&s, caller, &headers).await?;
            let sid = r.headers().get("location").and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit('/').next()).map(str::to_string);
            if let Some(sid) = sid { r.headers_mut().insert(HeaderName::from_static("upload-id"), HeaderValue::from_str(&sid).unwrap()); }
            Ok(r)
        }
        "HEAD" => head(&s, id()?, caller).await,
        "DELETE" => terminate(&s, id()?, caller).await,
        "PATCH" => {
            let sid = id()?;
            let ct = header(&headers, "content-type").unwrap_or("").to_ascii_lowercase();
            let limit = chunk_max_bytes() + 64 * 1024;
            let bytes = if ct.starts_with("multipart/form-data") {
                let req = axum::extract::Request::from_parts(parts, body);
                let mut mp = axum::extract::Multipart::from_request(req, &s).await.map_err(|e| bad(format!("Bad multipart body: {e}")))?;
                let mut chunk: Option<Bytes> = None;
                while let Some(field) = mp.next_field().await.map_err(|e| bad(format!("Bad multipart body: {e}")))? {
                    if field.file_name().is_some() && chunk.is_none() {
                        let b = field.bytes().await.map_err(|e| bad(format!("Bad multipart body: {e}")))?;
                        if b.len() > chunk_max_bytes() { return Err(bad("Chunk is too large.")); }
                        chunk = Some(b);
                    }
                }
                chunk.ok_or_else(|| bad("The chunk must be sent as a file part."))?
            } else {
                axum::body::to_bytes(body, limit).await.map_err(|_| bad("Chunk is too large."))?
            };
            let mut h = headers.clone();
            h.insert(axum::http::header::CONTENT_TYPE, HeaderValue::from_static("application/offset+octet-stream"));
            patch(&s, sid, caller, &h, bytes).await
        }
        _ => Err(bad("Unsupported method")),
    }
}

pub async fn options_capabilities() -> Response {
    let mut h = tus_headers();
    h.insert(HeaderName::from_static("tus-max-size"), HeaderValue::from_static("0")); // basket policy decides
    (StatusCode::NO_CONTENT, h).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upload_metadata_pairs() {
        let m = parse_upload_metadata("filename ZmlsZS50eHQ=,system_id c3lzX2hy,tags Wyl0ZXN0Il0=");
        assert_eq!(m["filename"], "file.txt");
        assert_eq!(m["system_id"], "sys_hr");
    }

    #[test]
    fn parses_empty_value() {
        let m = parse_upload_metadata("flag,filename ZmlsZS50eHQ=");
        assert_eq!(m.get("flag").map(String::as_str), Some(""));
        assert_eq!(m["filename"], "file.txt");
    }
}

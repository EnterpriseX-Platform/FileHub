//! Upload links ("file requests"): a link to a basket or folder that lets
//! people drop files in through a form, without seeing what's already there.

use std::sync::Arc;

use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;

use crate::acl::{self, EDIT};
use crate::auth::{AuthUser, MaybeAuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::handlers::{stage_field, upload_max_bytes, UploadFields};
use crate::state::AppState;

fn bad(m: impl Into<String>) -> ApiError { ApiError::BadRequest(m.into()) }

#[derive(Debug, Serialize, FromRow)]
pub struct LinkRow {
    token: String,
    system_id: String,
    folder_id: Option<String>,
    title: String,
    message: Option<String>,
    require_login: bool,
    expires_at: DateTime<Utc>,
    max_files: Option<i32>,
    upload_count: i32,
    created_by: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListQ { pub system_id: String, pub folder_id: Option<String> }

pub async fn list(State(s): State<Arc<AppState>>, user: AuthUser, Query(q): Query<ListQ>) -> ApiResult<Json<Vec<LinkRow>>> {
    let fld = q.folder_id.filter(|f| !f.is_empty());
    acl::require_folder(&s.db, &user.0, &q.system_id, fld.as_deref(), EDIT).await?;
    let rows: Vec<LinkRow> = match &fld {
        Some(f) => sqlx::query_as("SELECT token, system_id, folder_id, title, message, require_login, expires_at, max_files, upload_count, created_by, created_at FROM upload_links WHERE system_id = $1 AND folder_id = $2 AND revoked_at IS NULL AND expires_at > now() ORDER BY created_at DESC")
            .bind(&q.system_id).bind(f).fetch_all(&s.db).await?,
        None => sqlx::query_as("SELECT token, system_id, folder_id, title, message, require_login, expires_at, max_files, upload_count, created_by, created_at FROM upload_links WHERE system_id = $1 AND folder_id IS NULL AND revoked_at IS NULL AND expires_at > now() ORDER BY created_at DESC")
            .bind(&q.system_id).fetch_all(&s.db).await?,
    };
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct NewLink {
    pub system_id: String,
    pub folder_id: Option<String>,
    pub title: String,
    pub message: Option<String>,
    #[serde(default)]
    pub require_login: bool,
    pub expires_in_days: Option<i64>,
    pub max_files: Option<i32>,
}

pub async fn create(State(s): State<Arc<AppState>>, user: AuthUser, Json(b): Json<NewLink>) -> ApiResult<Json<LinkRow>> {
    let fld = b.folder_id.clone().filter(|f| !f.is_empty());
    if let Some(f) = &fld {
        let fs: Option<(String,)> = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL").bind(f).fetch_optional(&s.db).await?;
        if fs.map(|(x,)| x != b.system_id).unwrap_or(true) { return Err(bad("The folder is not in this basket.")); }
    }
    acl::require_folder(&s.db, &user.0, &b.system_id, fld.as_deref(), EDIT).await?;
    let title = b.title.trim();
    if title.is_empty() || title.len() > 200 { return Err(bad("Give the request a title (1–200 characters).")); }
    if let Some(m) = b.max_files { if !(1..=10000).contains(&m) { return Err(bad("Maximum files must be 1–10,000.")); } }
    let days = b.expires_in_days.unwrap_or(14).clamp(1, 90);
    let require_login = b.require_login || crate::basket::is_vault(&s.db, &b.system_id).await?;
    let token = crate::handlers::random_token();
    sqlx::query("INSERT INTO upload_links (token, system_id, folder_id, title, message, require_login, expires_at, max_files, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(&token).bind(&b.system_id).bind(&fld).bind(title).bind(b.message.as_deref().map(str::trim).filter(|m| !m.is_empty()))
        .bind(require_login).bind(Utc::now() + Duration::days(days)).bind(b.max_files).bind(&user.0.id)
        .execute(&s.db).await?;
    crate::basket::log_activity(&s.db, &user.0, "created upload link", title, "upload_link", None, &b.system_id).await;
    Ok(Json(sqlx::query_as("SELECT token, system_id, folder_id, title, message, require_login, expires_at, max_files, upload_count, created_by, created_at FROM upload_links WHERE token = $1")
        .bind(&token).fetch_one(&s.db).await?))
}

pub async fn revoke(State(s): State<Arc<AppState>>, user: AuthUser, Path(token): Path<String>) -> ApiResult<StatusCode> {
    let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT system_id, folder_id FROM upload_links WHERE token = $1 AND revoked_at IS NULL")
        .bind(&token).fetch_optional(&s.db).await?;
    let (sys, fld) = row.ok_or(ApiError::NotFound)?;
    acl::require_folder(&s.db, &user.0, &sys, fld.as_deref(), EDIT).await?;
    sqlx::query("UPDATE upload_links SET revoked_at = now() WHERE token = $1").bind(&token).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

// -----------------------------------------------------------------------------
// Public side (the token is the credential)
// -----------------------------------------------------------------------------

pub(crate) struct Live { pub(crate) sys: String, pub(crate) fld: Option<String>, title: String, message: Option<String>, pub(crate) require_login: bool, expires_at: DateTime<Utc>, pub(crate) left: Option<i32>, pub(crate) creator: User }

pub(crate) async fn live(s: &AppState, token: &str) -> Result<Live, ApiError> {
    let row: Option<(String, Option<String>, String, Option<String>, bool, DateTime<Utc>, Option<i32>, i32, String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT system_id, folder_id, title, message, require_login, expires_at, max_files, upload_count, created_by, revoked_at FROM upload_links WHERE token = $1",
    ).bind(token).fetch_optional(&s.db).await?;
    let Some((sys, fld, title, message, require_login, expires_at, max, count, creator, revoked)) = row else { return Err(ApiError::NotFound) };
    if revoked.is_some() || expires_at < Utc::now() { return Err(bad("This upload link has expired or was turned off. Ask the person who sent it for a new one.")); }
    let left = max.map(|m| (m - count).max(0));
    if left == Some(0) { return Err(bad("This upload link has already received all the files it accepts.")); }
    let creator: User = sqlx::query_as("SELECT * FROM users WHERE id = $1 AND status = 'active'").bind(&creator).fetch_optional(&s.db).await?
        .ok_or_else(|| bad("This upload link no longer works because its owner's account is closed."))?;
    // The owner must still be able to put files here.
    acl::require_folder(&s.db, &creator, &sys, fld.as_deref(), EDIT).await
        .map_err(|_| bad("This upload link no longer works because its owner lost access to the folder."))?;
    Ok(Live { sys, fld, title, message, require_login, expires_at, left, creator })
}

pub async fn public_meta(State(s): State<Arc<AppState>>, user: MaybeAuthUser, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let l = live(&s, &token).await?;
    let basket: String = sqlx::query_scalar("SELECT name FROM systems WHERE id = $1").bind(&l.sys).fetch_one(&s.db).await?;
    let folder: Option<String> = match &l.fld { Some(f) => sqlx::query_scalar("SELECT name FROM folders WHERE id = $1").bind(f).fetch_optional(&s.db).await?, None => None };
    let settings = crate::basket::load_settings(&s.db, &l.sys).await?;
    let pol = crate::basket::effective_policy(&s.db, &l.sys, l.fld.as_deref()).await?;
    Ok(Json(json!({
        "title": l.title, "message": l.message, "basket": basket, "folder": folder,
        "require_login": l.require_login, "signed_in": user.0.as_ref().map(|u| json!({"name": u.display_name, "email": u.email})),
        "expires_at": l.expires_at, "files_left": l.left, "requested_by": l.creator.display_name,
        "allowed_ext": pol.allowed_ext, "max_file_bytes": pol.max_file_bytes.map(|m| m.min(upload_max_bytes() as i64)).unwrap_or(upload_max_bytes() as i64),
        "fields": settings.fields, "categories": settings.categories,
    })))
}

/// Multipart: name, email, category_code, fields (JSON), file (repeatable).
pub async fn public_upload(State(s): State<Arc<AppState>>, user: MaybeAuthUser, Path(token): Path<String>, mut mp: Multipart) -> ApiResult<Json<Value>> {
    let l = live(&s, &token).await?;
    if l.require_login && user.0.is_none() { return Err(ApiError::Unauthorized); }
    crate::auth::upload_rate_limit_keyed(&s.db, &format!("ul:{token}"), 120).await?;

    let mut name: Option<String> = user.0.as_ref().map(|u| u.display_name.clone());
    let mut email: Option<String> = user.0.as_ref().map(|u| u.email.clone());
    let mut cat: Option<String> = None;
    let mut fields: Value = json!({});
    let mut staged = Vec::new();
    let res: Result<(), ApiError> = async {
        while let Some(mut f) = mp.next_field().await.map_err(|e| bad(e.to_string()))? {
            match f.name().unwrap_or("") {
                "name" => { let v = f.text().await.map_err(|e| bad(e.to_string()))?; if user.0.is_none() { name = Some(v.trim().to_string()).filter(|x| !x.is_empty()); } }
                "email" => { let v = f.text().await.map_err(|e| bad(e.to_string()))?; if user.0.is_none() { email = Some(v.trim().to_string()).filter(|x| !x.is_empty()); } }
                "category_code" => { cat = Some(f.text().await.map_err(|e| bad(e.to_string()))?).filter(|c| !c.is_empty()); }
                "fields" => { fields = serde_json::from_str(&f.text().await.map_err(|e| bad(e.to_string()))?).map_err(|_| bad("fields must be a JSON object"))?; }
                "file" => {
                    let fname = f.file_name().map(str::to_string).unwrap_or_else(|| "upload.bin".into());
                    let ct = f.content_type().map(str::to_string);
                    let st = stage_field(&s, &mut f, upload_max_bytes()).await?;
                    staged.push((fname, ct, st));
                }
                _ => {}
            }
        }
        Ok(())
    }.await;
    if let Err(e) = res { for (_, _, st) in staged { s.storage.discard(st).await; } return Err(e); }
    if staged.is_empty() { return Err(bad("Choose at least one file.")); }
    let prep = match check_submission(&s, &l, name, email, cat, fields, staged.len()).await {
        Ok(v) => v,
        Err(e) => { for (_, _, st) in staged { s.storage.discard(st).await; } return Err(e); }
    };
    Ok(Json(accept(&s, &token, &l, &prep, staged).await?))
}

/// Who sent it and what they filled in, checked before any file is kept.
pub(crate) struct Submission { pub(crate) who: String, pub(crate) email: Option<String>, pub(crate) cat: Option<String>, pub(crate) fields: Value }

pub(crate) async fn check_submission(s: &AppState, l: &Live, name: Option<String>, email: Option<String>, cat: Option<String>, mut fields: Value, n_files: usize) -> ApiResult<Submission> {
    let who = match &name { Some(n) if !n.trim().is_empty() && n.len() <= 200 => n.trim().to_string(), _ => return Err(bad("Enter your name.")) };
    if let Some(e) = &email { if !e.contains('@') || e.len() > 200 { return Err(bad("Enter a valid email address.")); } }
    if let Some(left) = l.left { if n_files as i32 > left { return Err(bad(format!("This link accepts {left} more file(s)."))); } }
    if fields.is_null() { fields = json!({}); }
    if !fields.is_object() { return Err(bad("fields must be a JSON object")); }
    // The form marks required fields; enforce them whoever owns the link.
    crate::basket::validate_fields(&s.db, &l.sys, &fields, true).await?;
    if let Some(c) = &cat { crate::basket::check_category(&s.db, &l.sys, Some(c)).await?; }
    // Submitter details travel with the file as fields.
    if let Some(o) = fields.as_object_mut() {
        o.insert("submitted_by".into(), json!(who));
        if let Some(e) = &email { o.insert("submitted_email".into(), json!(e)); }
    }
    Ok(Submission { who, email, cat, fields })
}

/// Keep staged files sent through a link (form or chunked upload), count them
/// against the link, and tell its owner.
pub(crate) async fn accept(s: &AppState, token: &str, l: &Live, sub: &Submission, staged: Vec<(String, Option<String>, crate::storage::Staged)>) -> ApiResult<Value> {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for (fname, ct, st) in staged {
        let up = UploadFields {
            name: Some(fname.clone()), system_id: Some(l.sys.clone()), folder_id: l.fld.clone(),
            content_type: ct, category_code: sub.cat.clone(), fields: Some(sub.fields.to_string()),
            owner: Some(format!("{} (upload link)", sub.who)), staged: Some(st), ..Default::default()
        };
        match crate::handlers::persist_upload_for(s, Some(l.creator.id.as_str()), up, Some(&l.creator)).await {
            Ok(file) => out.push(json!({ "name": file.name, "size_bytes": file.size_bytes })),
            Err(ApiError::BadRequest(m)) | Err(ApiError::PayloadTooLarge(m)) => errors.push(json!({ "name": fname, "error": m })),
            Err(e) => return Err(e),
        }
    }
    if !out.is_empty() {
        sqlx::query("UPDATE upload_links SET upload_count = upload_count + $1 WHERE token = $2").bind(out.len() as i32).bind(token).execute(&s.db).await?;
        let place = match &l.fld { Some(f) => format!("/folder/{f}"), None => format!("/b/{}", l.sys) };
        let _ = sqlx::query("INSERT INTO notifications (id, user_id, kind, title, body, link, created_at) VALUES ($1,$2,'upload_link',$3,$4,$5,now())")
            .bind(crate::Id::new()).bind(&l.creator.id)
            .bind(format!("{} uploaded {} file(s) to \"{}\"", sub.who, out.len(), l.title))
            .bind(sub.email.clone()).bind(place).execute(&s.db).await;
    }
    if out.is_empty() && !errors.is_empty() {
        return Err(bad(errors.iter().filter_map(|e| e["error"].as_str()).collect::<Vec<_>>().join(" ")));
    }
    Ok(json!({ "uploaded": out, "failed": errors }))
}

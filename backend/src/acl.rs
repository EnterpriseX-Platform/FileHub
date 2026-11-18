//! Resource-level access (View / Edit / Manage) on basket → folder → file.
//!
//! The rule lives in ONE place: the `fh_level` SQL function (migration 0018).
//! Rust only builds the caller's principal list and asks Postgres, so list
//! queries and single-resource checks can never disagree.
//!
//! Principals are plain strings, the same strings stored in `acl_entries.principal`:
//!   user:<id> · org:<org id> · role:<admin|editor|viewer> · iamx:<IAMX ROLE> · everyone

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use crate::Id;

use crate::auth::{AuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const VIEW: i32 = 1;
pub const EDIT: i32 = 2;
pub const MANAGE: i32 = 3;

pub fn level_name(l: i32) -> &'static str {
    match l { 3 => "manage", 2 => "edit", 1 => "view", _ => "none" }
}

fn level_from(s: &str) -> Option<i16> {
    match s { "view" => Some(1), "edit" => Some(2), "manage" => Some(3), _ => None }
}

/// Every principal string the caller matches.
pub async fn principals(db: &PgPool, user: &User) -> Result<Vec<String>, ApiError> {
    let row: Option<(Option<String>, Vec<String>)> = sqlx::query_as(
        "SELECT default_org_id, iamx_roles FROM users WHERE id = $1",
    )
    .bind(&user.id)
    .fetch_optional(db)
    .await?;
    let mut p = vec![
        format!("user:{}", user.id),
        format!("role:{}", user.role),
        "everyone".to_string(),
    ];
    if let Some((org, roles)) = row {
        if let Some(o) = org { p.push(format!("org:{o}")); }
        for r in roles { p.push(format!("iamx:{}", r.to_uppercase())); }
    }
    Ok(p)
}

#[derive(FromRow)]
struct FileRef {
    system_id: String,
    folder_id: Option<String>,
    created_by: Option<String>,
}

pub async fn file_level(db: &PgPool, user: &User, file_id: Id) -> Result<i32, ApiError> {
    let f: FileRef = sqlx::query_as(
        "SELECT system_id, folder_id, created_by FROM files WHERE id = $1",
    )
    .bind(file_id)
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if personal_block(db, user, &f.system_id).await? { return Ok(0); }
    let p = principals(db, user).await?;
    Ok(sqlx::query_scalar("SELECT fh_level($1,$2,$3,$4,$5,$6,$7)")
        .bind(&p).bind(&user.role).bind(&f.system_id).bind(&f.folder_id)
        .bind(file_id).bind(&f.created_by).bind(&user.id)
        .fetch_one(db).await?)
}

pub async fn folder_level(db: &PgPool, user: &User, system_id: &str, folder_id: Option<&str>) -> Result<i32, ApiError> {
    if personal_block(db, user, system_id).await? { return Ok(0); }
    let p = principals(db, user).await?;
    Ok(sqlx::query_scalar("SELECT fh_level($1,$2,$3,$4,NULL,NULL,$5)")
        .bind(&p).bind(&user.role).bind(system_id).bind(folder_id).bind(&user.id)
        .fetch_one(db).await?)
}

/// Someone else's personal drive: no access whatever the grants say
/// (owner = Manage via `owner_manage`, admins bypass in fh_level).
async fn personal_block(db: &PgPool, user: &User, system_id: &str) -> Result<bool, ApiError> {
    if user.role == "admin" { return Ok(false); }
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT system_type, owner_user_id FROM systems WHERE id = $1",
    ).bind(system_id).fetch_optional(db).await?;
    Ok(matches!(row, Some((t, o)) if t == "personal" && o.as_deref() != Some(&user.id)))
}

/// Read: 404 on no access (do not leak existence). Write: 403.
pub async fn require_file(db: &PgPool, user: &User, file_id: Id, need: i32) -> Result<i32, ApiError> {
    let l = file_level(db, user, file_id).await?;
    if l >= need { return Ok(l); }
    Err(if l == 0 { ApiError::NotFound } else { ApiError::Forbidden })
}

pub async fn require_folder(db: &PgPool, user: &User, system_id: &str, folder_id: Option<&str>, need: i32) -> Result<i32, ApiError> {
    let l = folder_level(db, user, system_id, folder_id).await?;
    if l >= need { return Ok(l); }
    Err(if l == 0 { ApiError::NotFound } else { ApiError::Forbidden })
}

/// SQL fragment that keeps only rows the caller can view. `alias` is the files
/// table alias; `pn`, `rn`, `un` are the bind positions of principals, role, user id.
pub fn view_filter_sql(alias: &str, pn: usize, rn: usize, un: usize) -> String {
    format!(" AND fh_level(${pn}, ${rn}, {a}.system_id, {a}.folder_id, {a}.id, {a}.created_by, ${un}) >= 1", a = alias)
}

/// What the UI may offer for a level (the API re-checks every call).
pub fn allowed_actions(level: i32) -> Vec<&'static str> {
    let mut a = Vec::new();
    if level >= VIEW { a.extend(["view", "download", "add_to_download_list"]); }
    if level >= EDIT { a.extend(["edit", "upload_version", "move", "rename", "change_status", "forward"]); }
    if level >= MANAGE { a.extend(["share", "public_link", "delete", "manage_access"]); }
    a
}

// -----------------------------------------------------------------------------
// HTTP
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct AclEntry {
    pub id: Id,
    pub resource_type: String,
    pub resource_id: String,
    pub principal: String,
    pub level: i16,
    pub expires_at: Option<DateTime<Utc>>,
    pub granted_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AclEntryView {
    #[serde(flatten)]
    pub entry: AclEntry,
    pub level_name: &'static str,
    pub principal_label: String,
    /// Where the grant comes from when it is inherited (folder/basket name).
    pub inherited_from: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AclResponse {
    pub resource_type: String,
    pub resource_id: String,
    pub inherit: bool,
    pub entries: Vec<AclEntryView>,
    pub inherited: Vec<AclEntryView>,
    pub my_level: &'static str,
}

/// Resolve (system_id, folder_id for chain start, file uuid) of a resource and check it exists.
async fn locate(db: &PgPool, rtype: &str, rid: &str) -> Result<(String, Option<String>, Option<Id>), ApiError> {
    match rtype {
        "basket" => {
            let ok: Option<(String,)> = sqlx::query_as("SELECT id FROM systems WHERE id = $1 AND deleted_at IS NULL")
                .bind(rid).fetch_optional(db).await?;
            ok.map(|(id,)| (id, None, None)).ok_or(ApiError::NotFound)
        }
        "folder" => {
            let r: Option<(String,)> = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL")
                .bind(rid).fetch_optional(db).await?;
            r.map(|(s,)| (s, Some(rid.to_string()), None)).ok_or(ApiError::NotFound)
        }
        "file" => {
            let id = crate::Id::parse_str(rid).map_err(|_| ApiError::NotFound)?;
            let r: Option<(String, Option<String>)> = sqlx::query_as(
                "SELECT system_id, folder_id FROM files WHERE id = $1 AND deleted_at IS NULL",
            ).bind(id).fetch_optional(db).await?;
            r.map(|(s, f)| (s, f, Some(id))).ok_or(ApiError::NotFound)
        }
        _ => Err(ApiError::BadRequest("resource type must be basket, folder or file".into())),
    }
}

async fn level_of(db: &PgPool, user: &User, rtype: &str, sys: &str, fld: Option<&str>, fid: Option<Id>) -> Result<i32, ApiError> {
    match (rtype, fid) {
        ("file", Some(id)) => file_level(db, user, id).await,
        _ => folder_level(db, user, sys, fld).await,
    }
}

async fn principal_label(db: &PgPool, p: &str) -> String {
    if let Some(id) = p.strip_prefix("user:") {
        return sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
            .bind(id).fetch_optional(db).await.ok().flatten().unwrap_or_else(|| id.to_string());
    }
    if let Some(id) = p.strip_prefix("org:") {
        return sqlx::query_scalar::<_, String>("SELECT name FROM orgs WHERE id = $1")
            .bind(id).fetch_optional(db).await.ok().flatten().unwrap_or_else(|| id.to_string());
    }
    if let Some(r) = p.strip_prefix("role:") { return format!("All {r}s"); }
    if let Some(r) = p.strip_prefix("iamx:") { return format!("IAMX role {r}"); }
    if p == "everyone" { return "Everyone signed in".into(); }
    p.to_string()
}

async fn view_of(db: &PgPool, e: AclEntry, inherited_from: Option<String>) -> AclEntryView {
    let label = principal_label(db, &e.principal).await;
    AclEntryView { level_name: level_name(e.level as i32), principal_label: label, inherited_from, entry: e }
}

pub async fn get_acl(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path((rtype, rid)): Path<(String, String)>,
) -> ApiResult<Json<AclResponse>> {
    let (sys, fld, fid) = locate(&s.db, &rtype, &rid).await?;
    let my = level_of(&s.db, &user.0, &rtype, &sys, fld.as_deref(), fid).await?;
    if my < VIEW { return Err(ApiError::NotFound); }

    let own: Vec<AclEntry> = sqlx::query_as(
        "SELECT * FROM acl_entries WHERE resource_type = $1 AND resource_id = $2 ORDER BY level DESC, created_at",
    ).bind(&rtype).bind(&rid).fetch_all(&s.db).await?;
    let mut entries = Vec::new();
    for e in own { entries.push(view_of(&s.db, e, None).await); }

    // Inherited grants: walk up the folder chain, then the basket.
    let mut inherited = Vec::new();
    let mut inherit = true;
    let mut cur: Option<String> = if rtype == "folder" {
        let (p, inh): (Option<String>, bool) = sqlx::query_as("SELECT parent_id, inherit FROM folders WHERE id = $1")
            .bind(&rid).fetch_one(&s.db).await?;
        inherit = inh;
        p
    } else if rtype == "file" { fld.clone() } else { None };
    if rtype != "basket" && inherit {
        let mut depth = 0;
        while let Some(f) = cur.clone() {
            if depth > 64 { break; }
            depth += 1;
            let (name, parent, inh): (String, Option<String>, bool) =
                sqlx::query_as("SELECT name, parent_id, inherit FROM folders WHERE id = $1")
                    .bind(&f).fetch_one(&s.db).await?;
            let rows: Vec<AclEntry> = sqlx::query_as(
                "SELECT * FROM acl_entries WHERE resource_type = 'folder' AND resource_id = $1",
            ).bind(&f).fetch_all(&s.db).await?;
            for e in rows { inherited.push(view_of(&s.db, e, Some(name.clone())).await); }
            if !inh { inherit = false; break; }
            cur = parent;
        }
        if inherit {
            let bname: String = sqlx::query_scalar("SELECT name FROM systems WHERE id = $1").bind(&sys).fetch_one(&s.db).await?;
            let rows: Vec<AclEntry> = sqlx::query_as(
                "SELECT * FROM acl_entries WHERE resource_type = 'basket' AND resource_id = $1",
            ).bind(&sys).fetch_all(&s.db).await?;
            for e in rows { inherited.push(view_of(&s.db, e, Some(bname.clone())).await); }
        }
    }
    let inherit_flag = if rtype == "folder" {
        sqlx::query_scalar("SELECT inherit FROM folders WHERE id = $1").bind(&rid).fetch_one(&s.db).await?
    } else { true };

    Ok(Json(AclResponse {
        resource_type: rtype, resource_id: rid, inherit: inherit_flag,
        entries, inherited, my_level: level_name(my),
    }))
}

#[derive(Debug, Deserialize)]
pub struct AclGrant {
    pub principal: String,
    pub level: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct PutAcl {
    pub entries: Vec<AclGrant>,
    /// Folders only: false = do not inherit grants from above.
    pub inherit: Option<bool>,
    /// Users added in this call get a notification (= Forward).
    #[serde(default)]
    pub notify: bool,
    pub message: Option<String>,
}

fn valid_principal(p: &str) -> bool {
    p == "everyone"
        || ["user:", "org:", "role:", "iamx:"].iter().any(|pre| p.strip_prefix(pre).map(|r| !r.is_empty() && r.len() <= 120).unwrap_or(false))
}

/// Replace the grants on one resource. Manage required.
pub async fn put_acl(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path((rtype, rid)): Path<(String, String)>,
    Json(body): Json<PutAcl>,
) -> ApiResult<Json<AclResponse>> {
    let (sys, fld, fid) = locate(&s.db, &rtype, &rid).await?;
    let my = level_of(&s.db, &user.0, &rtype, &sys, fld.as_deref(), fid).await?;
    if my < EDIT { return Err(if my == 0 { ApiError::NotFound } else { ApiError::Forbidden }); }
    // Edit = Forward only: keep every existing grant as it is and add people/agencies with View.
    if my < MANAGE {
        let cur: Vec<(String, i16, Option<DateTime<Utc>>)> = sqlx::query_as("SELECT principal, level, expires_at FROM acl_entries WHERE resource_type = $1 AND resource_id = $2")
            .bind(&rtype).bind(&rid).fetch_all(&s.db).await?;
        for (p, l, exp) in &cur {
            if !body.entries.iter().any(|g| &g.principal == p && level_from(&g.level) == Some(*l) && g.expires_at == *exp) {
                return Err(ApiError::BadRequest("With Edit access you can forward (add people with View) but not change or remove existing access.".into()));
            }
        }
        for g in &body.entries {
            if !cur.iter().any(|(p, _, _)| p == &g.principal) {
                if g.level != "view" {
                    return Err(ApiError::BadRequest("With Edit access you can give View only.".into()));
                }
                if !(g.principal.starts_with("user:") || g.principal.starts_with("org:")) {
                    return Err(ApiError::BadRequest("With Edit access you can forward to people or agencies only.".into()));
                }
            }
        }
        if body.inherit.is_some() && rtype == "folder" {
            let inh: bool = sqlx::query_scalar("SELECT inherit FROM folders WHERE id = $1").bind(&rid).fetch_one(&s.db).await?;
            if Some(inh) != body.inherit { return Err(ApiError::Forbidden); }
        }
    }

    let mut seen = std::collections::HashSet::new();
    for g in &body.entries {
        if !valid_principal(&g.principal) {
            return Err(ApiError::BadRequest(format!("Unknown principal \"{}\".", g.principal)));
        }
        if level_from(&g.level).is_none() {
            return Err(ApiError::BadRequest(format!("Level must be view, edit or manage (got \"{}\").", g.level)));
        }
        if !seen.insert(g.principal.clone()) {
            return Err(ApiError::BadRequest(format!("{} is listed twice.", g.principal)));
        }
    }

    let vault = crate::basket::is_vault(&s.db, &sys).await?;
    if vault && rtype != "basket" && body.entries.iter().any(|g| g.principal == "everyone" || g.principal.starts_with("role:")) {
        return Err(ApiError::BadRequest("Inside a Vault, share with named people, agencies or IAMX roles only.".into()));
    }
    if rtype == "basket" && vault {
        if !body.entries.iter().any(|g| g.level == "manage") { return Err(ApiError::BadRequest("A Vault must keep at least one person with Manage.".into())); }
        if body.entries.iter().any(|g| g.principal == "everyone" || g.principal.starts_with("role:")) {
            return Err(ApiError::BadRequest("A Vault is shared with named people, agencies or IAMX roles only — not everyone or a whole FileHub role.".into()));
        }
    }
    let before: Vec<String> = sqlx::query_scalar(
        "SELECT principal FROM acl_entries WHERE resource_type = $1 AND resource_id = $2",
    ).bind(&rtype).bind(&rid).fetch_all(&s.db).await?;

    let mut tx = s.db.begin().await?;
    sqlx::query("DELETE FROM acl_entries WHERE resource_type = $1 AND resource_id = $2")
        .bind(&rtype).bind(&rid).execute(&mut *tx).await?;
    for g in &body.entries {
        sqlx::query(
            r#"INSERT INTO acl_entries (id, system_id, resource_type, resource_id, principal, level, expires_at, granted_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
        )
        .bind(crate::Id::new()).bind(&sys).bind(&rtype).bind(&rid).bind(&g.principal)
        .bind(level_from(&g.level).unwrap()).bind(g.expires_at).bind(&user.0.id)
        .execute(&mut *tx).await?;
    }
    if rtype == "folder" {
        if let Some(inh) = body.inherit {
            sqlx::query("UPDATE folders SET inherit = $1 WHERE id = $2").bind(inh).bind(&rid).execute(&mut *tx).await?;
        }
    }
    // A basket that turns strict must not lock its own manager out.
    if rtype == "basket" && !body.entries.is_empty() && user.0.role != "admin" {
        let p = principals(&s.db, &user.0).await?;
        let still: Option<i16> = sqlx::query_scalar(
            "SELECT MAX(level) FROM acl_entries WHERE resource_type='basket' AND resource_id=$1 AND principal = ANY($2)",
        ).bind(&rid).bind(&p).fetch_one(&mut *tx).await?;
        if still.unwrap_or(0) < MANAGE as i16 {
            return Err(ApiError::BadRequest("Keep yourself as Manage, or you would lose access to this basket.".into()));
        }
    }
    tx.commit().await?;

    // Forward: notify users newly added.
    if body.notify {
        let target_name = resource_name(&s.db, &rtype, &rid).await?;
        for g in &body.entries {
            if before.contains(&g.principal) { continue; }
            if let Some(uid) = g.principal.strip_prefix("user:") {
                let msg = match body.message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                    Some(m) => format!("{} shared \"{}\" with you: {}", user.0.display_name, target_name, m),
                    None => format!("{} shared \"{}\" with you", user.0.display_name, target_name),
                };
                let _ = sqlx::query(
                    r#"INSERT INTO notifications (id, user_id, kind, title, body, link, created_at)
                       VALUES ($1,$2,'share',$3,$4,$5,now())"#,
                )
                .bind(crate::Id::new()).bind(uid).bind(&msg).bind(body.message.clone())
                .bind(match (rtype.as_str(), fid) {
                    ("file", Some(id)) => format!("/f/{id}"),
                    ("folder", _) => format!("/folder/{rid}"),
                    _ => format!("/b/{sys}"),
                })
                .execute(&s.db).await;
            }
        }
    }

    let _ = sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, created_at, actor_id)
           VALUES ($1,$2,'slate','access changed',$3,$4,$5,$6,now(),$7)"#,
    )
    .bind(crate::Id::new()).bind(&user.0.display_name).bind(&rid).bind(&rtype).bind(fid).bind(&sys).bind(&user.0.id)
    .execute(&s.db).await;

    get_acl(State(s), user, Path((rtype, rid))).await
}

async fn resource_name(db: &PgPool, rtype: &str, rid: &str) -> Result<String, ApiError> {
    Ok(match rtype {
        "basket" => sqlx::query_scalar("SELECT name FROM systems WHERE id = $1").bind(rid).fetch_one(db).await?,
        "folder" => sqlx::query_scalar("SELECT name FROM folders WHERE id = $1").bind(rid).fetch_one(db).await?,
        _ => {
            let id = crate::Id::parse_str(rid).map_err(|_| ApiError::NotFound)?;
            sqlx::query_scalar("SELECT name FROM files WHERE id = $1").bind(id).fetch_one(db).await?
        }
    })
}

#[derive(Debug, Serialize)]
pub struct Effective {
    pub level: &'static str,
    pub actions: Vec<&'static str>,
}

pub async fn effective(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path((rtype, rid)): Path<(String, String)>,
) -> ApiResult<Json<Effective>> {
    let (sys, fld, fid) = locate(&s.db, &rtype, &rid).await?;
    let l = level_of(&s.db, &user.0, &rtype, &sys, fld.as_deref(), fid).await?;
    if l < VIEW { return Err(ApiError::NotFound); }
    Ok(Json(Effective { level: level_name(l), actions: allowed_actions(l) }))
}

// -----------------------------------------------------------------------------
// Principal search — people, agencies, roles for the Share dialog
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PrincipalQuery { pub q: Option<String> }

#[derive(Debug, Serialize)]
pub struct Principal { pub principal: String, pub label: String, pub kind: &'static str, pub detail: Option<String> }

pub async fn search_principals(
    State(s): State<Arc<AppState>>,
    _user: AuthUser,
    Query(q): Query<PrincipalQuery>,
) -> ApiResult<Json<Vec<Principal>>> {
    let term = q.q.unwrap_or_default().trim().to_string();
    let like = format!("%{}%", term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut out = Vec::new();
    let users: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, display_name, email FROM users WHERE status = 'active' AND (display_name ILIKE $1 OR email ILIKE $1) ORDER BY display_name LIMIT 10",
    ).bind(&like).fetch_all(&s.db).await?;
    for (id, name, email) in users {
        out.push(Principal { principal: format!("user:{id}"), label: name, kind: "user", detail: Some(email) });
    }
    let orgs: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, name, code FROM orgs WHERE name ILIKE $1 OR code ILIKE $1 ORDER BY name LIMIT 10",
    ).bind(&like).fetch_all(&s.db).await?;
    for (id, name, code) in orgs {
        out.push(Principal { principal: format!("org:{id}"), label: name, kind: "agency", detail: Some(code) });
    }
    for r in ["admin", "editor", "viewer"] {
        if term.is_empty() || r.contains(&term.to_lowercase()) {
            out.push(Principal { principal: format!("role:{r}"), label: format!("All {r}s"), kind: "role", detail: None });
        }
    }
    let iamx: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT r FROM users, unnest(iamx_roles) r WHERE r ILIKE $1 ORDER BY r LIMIT 10",
    ).bind(&like).fetch_all(&s.db).await?;
    for (r,) in iamx {
        out.push(Principal { principal: format!("iamx:{}", r.to_uppercase()), label: format!("IAMX role {}", r.to_uppercase()), kind: "iamx_role", detail: None });
    }
    if term.is_empty() || "everyone".contains(&term.to_lowercase()) {
        out.push(Principal { principal: "everyone".into(), label: "Everyone signed in".into(), kind: "everyone", detail: None });
    }
    Ok(Json(out))
}

// -----------------------------------------------------------------------------
// "Shared with me" and "Awaiting my review"
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct MineRow {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub system_id: String,
    pub basket_name: String,
    pub folder_id: Option<String>,
    pub status: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<DateTime<Utc>>,
    pub level: i16,
    pub granted_by: Option<String>,
}

/// Files and folders given to the caller directly (user grant) or through their
/// agency / IAMX role — not basket-wide grants, which show up as baskets.
pub async fn shared_with_me(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Vec<MineRow>>> {
    let p: Vec<String> = principals(&s.db, &user.0).await?.into_iter()
        .filter(|x| x != "everyone" && !x.starts_with("role:")).collect();
    let rows: Vec<MineRow> = sqlx::query_as(
        r#"SELECT 'file' AS kind, f.id::text AS id, f.name, f.system_id, sy.name AS basket_name, f.folder_id,
                  f.status, f.size_bytes, f.modified_at, MAX(a.level) AS level, MAX(u.display_name) AS granted_by
             FROM acl_entries a JOIN files f ON f.id::text = a.resource_id AND a.resource_type = 'file'
             JOIN systems sy ON sy.id = f.system_id LEFT JOIN users u ON u.id = a.granted_by
            WHERE a.principal = ANY($1) AND f.deleted_at IS NULL AND (a.expires_at IS NULL OR a.expires_at > now())
            GROUP BY f.id, sy.name
           UNION ALL
           SELECT 'folder', fo.id, fo.name, fo.system_id, sy.name, fo.parent_id, NULL, NULL, fo.created_at,
                  MAX(a.level), MAX(u.display_name)
             FROM acl_entries a JOIN folders fo ON fo.id = a.resource_id AND a.resource_type = 'folder'
             JOIN systems sy ON sy.id = fo.system_id LEFT JOIN users u ON u.id = a.granted_by
            WHERE a.principal = ANY($1) AND fo.deleted_at IS NULL AND (a.expires_at IS NULL OR a.expires_at > now())
            GROUP BY fo.id, sy.name
            ORDER BY 9 DESC NULLS LAST LIMIT 500"#,
    ).bind(&p).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

/// Files whose current status has a move that needs Manage (an approval) and the
/// caller has Manage on — excluding the caller's own uploads.
pub async fn awaiting_review(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Vec<MineRow>>> {
    let p = principals(&s.db, &user.0).await?;
    let scope = crate::auth::effective_system_ids(&s.db, &user.0).await?;
    let scope_sql = if scope.is_some() { "              AND f.system_id = ANY($4)" } else { "" };
    let sql = format!(
        r#"SELECT 'file' AS kind, f.id::text AS id, f.name, f.system_id, sy.name AS basket_name, f.folder_id,
                  f.status, f.size_bytes, f.modified_at, 3::smallint AS level, NULL::text AS granted_by
             FROM files f JOIN systems sy ON sy.id = f.system_id
            WHERE f.deleted_at IS NULL
{scope_sql}
              AND f.created_by IS DISTINCT FROM $3
              AND EXISTS (SELECT 1 FROM basket_statuses st
                           WHERE st.system_id = f.system_id AND st.code = f.status AND st.awaits_review)
              AND fh_level($1, $2, f.system_id, f.folder_id, f.id, f.created_by, $3) >= 3
            ORDER BY f.modified_at DESC LIMIT 500"#);
    let mut qy = sqlx::query_as::<_, MineRow>(&sql).bind(&p).bind(&user.0.role).bind(&user.0.id);
    if let Some(sc) = &scope { qy = qy.bind(sc); }
    let rows = qy.fetch_all(&s.db).await?;
    Ok(Json(rows))
}

// -----------------------------------------------------------------------------
// Set-based visibility for list queries
// -----------------------------------------------------------------------------
//
// `fh_level()` per row is exact but costs a function call and several lookups
// per file — a list over 17k files took ~0.6 s and collapsed under load. Lists
// instead compute, once per request, which baskets (basket level), folders and
// files the caller may view, using the same rule as fh_level:
//   basket level = basket grants if the basket has any, else the role;
//   folder level = max(own grants, parent level if it inherits);
//   file visible = folder/basket level ≥ View, or a file grant, or own upload.
// Single-resource checks keep using fh_level().

pub struct Vis { pub baskets: Vec<String>, pub folders: Vec<String>, pub files: Vec<String>, pub uid: String }

pub async fn visibility(db: &PgPool, user: &User, scope: &[String]) -> Result<Vis, ApiError> {
    let p = principals(db, user).await?;
    let role_lvl: i32 = match user.role.as_str() { "admin" => 3, "editor" => 2, "viewer" => 1, _ => 0 };
    let row: (Option<Vec<String>>, Option<Vec<String>>, Option<Vec<String>>) = sqlx::query_as(
        r#"WITH RECURSIVE
             b AS (
               SELECT s.id,
                      CASE WHEN EXISTS (SELECT 1 FROM acl_entries a WHERE a.resource_type = 'basket' AND a.resource_id = s.id)
                           THEN COALESCE((SELECT MAX(a.level) FROM acl_entries a WHERE a.resource_type = 'basket' AND a.resource_id = s.id
                                            AND a.principal = ANY($1) AND (a.expires_at IS NULL OR a.expires_at > now())), 0)
                           ELSE $2::int END
                      + CASE WHEN s.system_type = 'personal' AND s.owner_user_id = $3 THEN 3 ELSE 0 END AS lvl
                 FROM systems s WHERE s.id = ANY($4)),
             g AS (
               SELECT resource_id, MAX(level)::int AS lvl FROM acl_entries
                WHERE resource_type = 'folder' AND principal = ANY($1) AND (expires_at IS NULL OR expires_at > now()) AND system_id = ANY($4)
                GROUP BY resource_id),
             t AS (
               SELECT f.id, GREATEST(COALESCE(g.lvl, 0), CASE WHEN f.inherit THEN b.lvl ELSE 0 END) AS lvl
                 FROM folders f JOIN b ON b.id = f.system_id LEFT JOIN g ON g.resource_id = f.id
                WHERE f.parent_id IS NULL AND f.deleted_at IS NULL
               UNION ALL
               SELECT f.id, GREATEST(COALESCE(g.lvl, 0), CASE WHEN f.inherit THEN t.lvl ELSE 0 END)
                 FROM folders f JOIN t ON f.parent_id = t.id LEFT JOIN g ON g.resource_id = f.id
                WHERE f.deleted_at IS NULL)
           SELECT (SELECT array_agg(id) FROM b WHERE lvl >= 1),
                  (SELECT array_agg(id) FROM t WHERE lvl >= 1),
                  (SELECT array_agg(DISTINCT resource_id) FROM acl_entries
                    WHERE resource_type = 'file' AND principal = ANY($1) AND (expires_at IS NULL OR expires_at > now()) AND system_id = ANY($4))"#,
    ).bind(&p).bind(role_lvl).bind(&user.id).bind(scope).fetch_one(db).await?;
    Ok(Vis { baskets: row.0.unwrap_or_default(), folders: row.1.unwrap_or_default(), files: row.2.unwrap_or_default(), uid: user.id.clone() })
}

/// WHERE fragment for files alias `a`; `b`, `f`, `fi`, `u` are bind positions
/// of the baskets, folders, files arrays and the user id array (`($u)[1]`).
pub fn vis_sql(a: &str, b: usize, f: usize, fi: usize, u: usize) -> String {
    format!(" AND (({a}.folder_id IS NULL AND {a}.system_id = ANY(${b})) OR {a}.folder_id = ANY(${f}) OR {a}.id::text = ANY(${fi}) OR {a}.created_by = (${u})[1])")
}

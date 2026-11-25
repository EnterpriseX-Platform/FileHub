//! Fiscal-year folder layout for one basket.
//!
//! ```text
//! <fiscal year>/<parent organisation>/<organisation>/<main group>/<source system>[/<stage>]
//! <fiscal year>/<support group>            (support systems, one folder, tagged system:<CODE>)
//! ```
//!
//! Turned on per basket by a `layout_settings` row. Source systems
//! (`layout_sources`) say whether they belong to the main group or the support
//! group and whether their folder keeps every version. Stages
//! (`layout_stages`) are kept per fiscal year; a stage folder is named
//! "<code> <name>" and is renamed when the stage's name changes.
//!
//! Years (the current and the next one) and the support folder are created
//! ahead by the worker; everything below is created when the first file
//! arrives. Access: an organisation's folder (and its parent's) is granted to
//! that organisation only; in the support folder each file is granted to the
//! organisation that uploaded it. Admins see everything.

use std::sync::Arc;

use axum::{extract::{Path, State}, Json};
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::{AdminUser, User};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Settings {
    pub system_id: String,
    pub current_year: Option<i32>,
    pub fy_start_month: i32,
    pub year_offset: i32,
    pub main_name: String,
    pub support_name: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Source {
    pub code: String,
    pub folder_name: String,
    pub grp: String,
    pub keep_versions: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Org {
    pub id: String,
    pub code: String,
    pub name: String,
    pub parent_id: Option<String>,
}

/// Where a placed file goes.
pub struct Placement {
    pub system_id: String,
    pub folder_id: String,
    pub org_id: Option<String>,
    pub tags: Vec<String>,
    /// Grant the file to this organisation (support folder).
    pub grant_org: Option<String>,
}

/// What the caller told us about a file.
#[derive(Default, Debug, Clone)]
pub struct Hint {
    pub org_code: Option<String>,
    pub source_code: Option<String>,
    pub fiscal_year: Option<i32>,
    pub stage_code: Option<String>,
    pub stage_name: Option<String>,
    pub referer: Option<String>,
}

impl Hint {
    pub fn is_empty(&self) -> bool { self.org_code.is_none() && self.source_code.is_none() }
}

impl Settings {
    /// Folder for files whose organisation is not known yet.
    pub fn unassigned_name(&self) -> String { "Unassigned organisation".into() }
}

/// The basket that has a layout (one per deployment).
pub async fn settings(db: &PgPool) -> Option<Settings> {
    sqlx::query_as("SELECT system_id, current_year, fy_start_month, year_offset, main_name, support_name FROM layout_settings WHERE enabled ORDER BY system_id LIMIT 1")
        .fetch_optional(db).await.ok().flatten()
}

/// The fiscal year files are filed under now: the configured one, else from the date.
pub fn current_year(st: &Settings) -> i32 {
    if let Some(y) = st.current_year { return y; }
    let now = chrono::Utc::now();
    let y = now.year() + if now.month() as i32 >= st.fy_start_month && st.fy_start_month > 1 { 1 } else { 0 };
    y + st.year_offset
}

async fn source_for(db: &PgPool, sys: &str, h: &Hint) -> ApiResult<Option<Source>> {
    if let Some(code) = h.source_code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        return Ok(sqlx::query_as("SELECT code, folder_name, grp, keep_versions FROM layout_sources WHERE system_id = $1 AND upper(code) = upper($2)")
            .bind(sys).bind(code).fetch_optional(db).await?);
    }
    let referer = h.referer.as_deref().unwrap_or("").to_lowercase();
    if referer.is_empty() { return Ok(None); }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT code, lower(p) FROM layout_sources, unnest(path_prefixes) p WHERE system_id = $1",
    ).bind(sys).fetch_all(db).await?;
    let mut rows = rows;
    rows.sort_by_key(|(_, p)| std::cmp::Reverse(p.len()));
    let Some(code) = rows.into_iter().find(|(_, p)| crate::connectors::prefix_matches(&referer, p))
        .map(|(c, _)| c) else { return Ok(None) };
    Ok(sqlx::query_as("SELECT code, folder_name, grp, keep_versions FROM layout_sources WHERE system_id = $1 AND code = $2")
        .bind(sys).bind(code).fetch_optional(db).await?)
}

async fn org_by_code(db: &PgPool, code: &str) -> ApiResult<Option<Org>> {
    Ok(sqlx::query_as("SELECT id, code, name, parent_id FROM orgs WHERE code = $1 AND status = 'Active'")
        .bind(code.trim()).fetch_optional(db).await?)
}

async fn org_by_id(db: &PgPool, id: &str) -> ApiResult<Option<Org>> {
    Ok(sqlx::query_as("SELECT id, code, name, parent_id FROM orgs WHERE id = $1")
        .bind(id).fetch_optional(db).await?)
}

/// A name folders accept: no slashes (a look-alike is used), at most 200 characters.
pub fn folder_name(n: &str) -> String {
    let n: String = n.trim().replace('/', "\u{FF0F}").replace('\\', "\u{FF3C}").chars().take(200).collect();
    if n.is_empty() || n == "." || n == ".." { "_".into() } else { n }
}

/// Options for a folder the layout creates.
#[derive(Default, Clone, Copy)]
struct FolderOpts { inherit: bool, keep_versions: Option<bool> }

/// The folder for `key` under `parent`, created (encrypted) when missing; its name follows `name`.
async fn ensure(db: &PgPool, sys: &str, key: &str, parent: Option<&str>, name: &str, org: Option<&str>, o: FolderOpts) -> ApiResult<String> {
    let name = folder_name(name);
    let name = name.as_str();
    if let Some((id, cur)) = sqlx::query_as::<_, (String, String)>(
        "SELECT f.id, f.name FROM layout_folders l JOIN folders f ON f.id = l.folder_id WHERE l.system_id = $1 AND l.path_key = $2 AND f.deleted_at IS NULL",
    ).bind(sys).bind(key).fetch_optional(db).await? {
        if cur != name { sqlx::query("UPDATE folders SET name = $2 WHERE id = $1").bind(&id).bind(name).execute(db).await?; }
        return Ok(id);
    }
    let id = crate::new_id();
    let mut tx = db.begin().await?;
    sqlx::query("INSERT INTO folders (id, system_id, org_id, parent_id, name, owner, encrypted, inherit) VALUES ($1,$2,$3,$4,$5,'FileHub',true,$6)")
        .bind(&id).bind(sys).bind(org).bind(parent).bind(name).bind(o.inherit).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO basket_policies (scope_type, scope_id, encrypt, keep_versions) VALUES ('folder',$1,true,$2) ON CONFLICT (scope_type, scope_id) DO NOTHING")
        .bind(&id).bind(o.keep_versions).execute(&mut *tx).await?;
    // Two uploads racing for the same folder: the first one wins, the other reuses it.
    let won = sqlx::query("INSERT INTO layout_folders (system_id, path_key, folder_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(sys).bind(key).bind(&id).execute(&mut *tx).await?.rows_affected() == 1;
    if won { tx.commit().await?; return Ok(id); }
    tx.rollback().await?;
    Ok(sqlx::query_scalar("SELECT folder_id FROM layout_folders WHERE system_id = $1 AND path_key = $2").bind(sys).bind(key).fetch_one(db).await?)
}

async fn grant(db: &PgPool, sys: &str, rtype: &str, rid: &str, org_id: &str, level: i16) -> ApiResult<()> {
    grant_to(db, sys, rtype, rid, &format!("org:{org_id}"), level).await
}

async fn grant_to(db: &PgPool, sys: &str, rtype: &str, rid: &str, principal: &str, level: i16) -> ApiResult<()> {
    sqlx::query("INSERT INTO acl_entries (id, system_id, resource_type, resource_id, principal, level, granted_by) VALUES ($1,$2,$3,$4,$5,$6,'layout')
                 ON CONFLICT (resource_type, resource_id, principal) DO NOTHING")
        .bind(crate::new_id()).bind(sys).bind(rtype).bind(rid).bind(principal).bind(level).execute(db).await?;
    Ok(())
}

async fn year_folder(db: &PgPool, st: &Settings, year: i32) -> ApiResult<String> {
    ensure(db, &st.system_id, &format!("y:{year}"), None, &year.to_string(), None, FolderOpts { inherit: true, keep_versions: None }).await
}

async fn support_folder(db: &PgPool, st: &Settings, year: i32) -> ApiResult<String> {
    let y = year_folder(db, st, year).await?;
    ensure(db, &st.system_id, &format!("y:{year}/support"), Some(&y), &st.support_name, None,
           FolderOpts { inherit: false, keep_versions: Some(true) }).await
}

/// The stage's folder name ("<code> <name>"), from the year's list or the latest earlier one.
async fn stage_name(db: &PgPool, sys: &str, year: i32, source: &str, code: &str, sent: Option<&str>) -> ApiResult<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM layout_stages WHERE system_id = $1 AND source_code = $2 AND code = $3 AND active AND fiscal_year <= $4 ORDER BY fiscal_year DESC LIMIT 1",
    ).bind(sys).bind(source).bind(code).bind(year).fetch_optional(db).await?;
    let name = row.map(|r| r.0).or_else(|| sent.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string));
    Ok(name.map(|n| format!("{code} {n}")))
}

/// Where a file with these hints goes, creating folders on the way. `None`
/// = the layout doesn't apply (no layout, unknown source, or a main-group file
/// without a known organisation): the caller keeps its usual behaviour.
pub async fn place(db: &PgPool, h: &Hint, uploader: Option<&User>) -> ApiResult<Option<Placement>> {
    if h.is_empty() && uploader.is_none() { return Ok(None); }
    let Some(st) = settings(db).await else { return Ok(None) };
    let Some(src) = source_for(db, &st.system_id, h).await? else { return Ok(None) };
    let year = h.fiscal_year.filter(|y| (2000..=3000).contains(y)).unwrap_or_else(|| current_year(&st));

    let mut org = match h.org_code.as_deref().filter(|c| !c.trim().is_empty()) {
        Some(c) => org_by_code(db, c).await?,
        None => None,
    };
    if org.is_none() {
        if let Some(u) = uploader {
            let def: Option<String> = sqlx::query_scalar("SELECT default_org_id FROM users WHERE id = $1 AND connector_id IS NULL")
                .bind(&u.id).fetch_optional(db).await?.flatten();
            if let Some(d) = def { org = org_by_id(db, &d).await?; }
        }
    }
    let mut tags = vec![format!("system:{}", src.code), format!("fy:{year}")];
    // The account a source system uploads as needs Edit on the closed folders it files into.
    let mut account: Option<String> = None;
    if let Some(u) = uploader {
        if crate::connectors::is_connector_user(db, &u.id).await { account = Some(format!("user:{}", u.id)); }
    }

    if src.grp == "support" {
        let folder = support_folder(db, &st, year).await?;
        if let Some(a) = &account { grant_to(db, &st.system_id, "folder", &folder, a, 2).await?; }
        if let Some(o) = &org { tags.push(format!("org:{}", o.code)); }
        return Ok(Some(Placement { system_id: st.system_id, folder_id: folder, org_id: org.as_ref().map(|o| o.id.clone()), tags, grant_org: org.map(|o| o.id) }));
    }

    let sys = st.system_id.as_str();
    let Some(org) = org else {
        // The source system is known but not the organisation (a back-end or a
        // shared account sent no orgCode): file it under the year anyway, in a
        // closed folder only admins and the uploading account see, never loose
        // in the basket root.
        let y = year_folder(db, &st, year).await?;
        let nf = ensure(db, sys, &format!("y:{year}/o:_none"), Some(&y), &st.unassigned_name(), None, FolderOpts { inherit: false, keep_versions: None }).await?;
        if let Some(a) = &account { grant_to(db, sys, "folder", &nf, a, 2).await?; }
        let mf = ensure(db, sys, &format!("y:{year}/o:_none/main"), Some(&nf), &st.main_name, None, FolderOpts { inherit: true, keep_versions: None }).await?;
        let folder = ensure(db, sys, &format!("y:{year}/o:_none/main/s:{}", src.code), Some(&mf), &src.folder_name, None,
                            FolderOpts { inherit: true, keep_versions: Some(src.keep_versions) }).await?;
        tags.push("org:unassigned".into());
        return Ok(Some(Placement { system_id: st.system_id.clone(), folder_id: folder, org_id: None, tags, grant_org: None }));
    };
    let parent = match &org.parent_id { Some(p) => org_by_id(db, p).await?, None => None };
    let y = year_folder(db, &st, year).await?;
    // Organisation folders are closed to everyone but that organisation (and admins).
    let (under, base) = match &parent {
        Some(p) => {
            let pf = ensure(db, sys, &format!("y:{year}/o:{}", p.code), Some(&y), &p.name, Some(&p.id), FolderOpts { inherit: false, keep_versions: None }).await?;
            grant(db, sys, "folder", &pf, &org.id, 1).await?;
            if let Some(a) = &account { grant_to(db, sys, "folder", &pf, a, 2).await?; }
            (pf, format!("y:{year}/o:{}", p.code))
        }
        None => (y.clone(), format!("y:{year}")),
    };
    let of = ensure(db, sys, &format!("{base}/o:{}", org.code), Some(&under), &org.name, Some(&org.id), FolderOpts { inherit: false, keep_versions: None }).await?;
    grant(db, sys, "folder", &of, &org.id, 2).await?;
    if let Some(a) = &account { grant_to(db, sys, "folder", &of, a, 2).await?; }
    let base = format!("{base}/o:{}", org.code);
    let mf = ensure(db, sys, &format!("{base}/main"), Some(&of), &st.main_name, Some(&org.id), FolderOpts { inherit: true, keep_versions: None }).await?;
    let base = format!("{base}/main");
    let mut folder = ensure(db, sys, &format!("{base}/s:{}", src.code), Some(&mf), &src.folder_name, Some(&org.id),
                            FolderOpts { inherit: true, keep_versions: Some(src.keep_versions) }).await?;
    if let Some(code) = h.stage_code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        if let Some(name) = stage_name(db, sys, year, &src.code, code, h.stage_name.as_deref()).await? {
            folder = ensure(db, sys, &format!("{base}/s:{}/st:{code}", src.code), Some(&folder), &name, Some(&org.id),
                            FolderOpts { inherit: true, keep_versions: Some(src.keep_versions) }).await?;
            tags.push(format!("stage:{code}"));
        }
    }
    tags.push(format!("org:{}", org.code));
    Ok(Some(Placement { system_id: st.system_id, folder_id: folder, org_id: Some(org.id), tags, grant_org: None }))
}

/// After the file row exists: grant it to the uploading organisation (support folder).
pub async fn after_upload(db: &PgPool, p: &Placement, file_id: &str) -> ApiResult<()> {
    if let Some(o) = &p.grant_org { grant(db, &p.system_id, "file", file_id, o, 1).await?; }
    Ok(())
}

/// Years ahead (current + next) and their support folders.
pub async fn prepare(db: &PgPool) -> ApiResult<usize> {
    let Some(st) = settings(db).await else { return Ok(0) };
    let y = current_year(&st);
    for year in [y, y + 1] { support_folder(db, &st, year).await?; }
    Ok(2)
}

/// Rename stage folders whose stage name changed (all years).
pub async fn rename_stages(db: &PgPool, sys: &str) -> ApiResult<u64> {
    let r = sqlx::query(
        r#"UPDATE folders f SET name = left(replace(replace(s.code || ' ' || s.name, '/', U&'\FF0F'), '\', U&'\FF3C'), 200)
             FROM layout_folders l
             JOIN layout_stages s ON s.system_id = l.system_id AND s.active
                  AND l.path_key LIKE 'y:' || s.fiscal_year || '/%/s:' || s.source_code || '/st:' || s.code
            WHERE l.system_id = $1 AND f.id = l.folder_id AND f.deleted_at IS NULL
              AND f.name <> left(replace(replace(s.code || ' ' || s.name, '/', U&'\FF0F'), '\', U&'\FF3C'), 200)"#,
    ).bind(sys).execute(db).await?;
    Ok(r.rows_affected())
}

pub async fn run_worker(state: Arc<AppState>) {
    loop {
        if let Err(e) = prepare(&state.db).await { tracing::warn!("layout: couldn't prepare the year folders: {e:?}"); }
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

// ── Admin REST ──

#[derive(Debug, Deserialize)]
pub struct StageIn {
    pub source: String,
    pub code: String,
    pub name: String,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub sort: i32,
}
fn yes() -> bool { true }

/// GET /api/layout — settings, sources and stages of the layout basket.
pub async fn get(State(s): State<Arc<AppState>>, _: AdminUser) -> ApiResult<Json<Value>> {
    let Some(st) = settings(&s.db).await else { return Ok(Json(json!({ "enabled": false }))) };
    let sources: Vec<(String, String, String, bool, Vec<String>)> = sqlx::query_as(
        "SELECT code, folder_name, grp, keep_versions, path_prefixes FROM layout_sources WHERE system_id = $1 ORDER BY grp, code",
    ).bind(&st.system_id).fetch_all(&s.db).await?;
    let stages: Vec<(i32, String, String, String, bool)> = sqlx::query_as(
        "SELECT fiscal_year, source_code, code, name, active FROM layout_stages WHERE system_id = $1 ORDER BY fiscal_year, source_code, sort, code",
    ).bind(&st.system_id).fetch_all(&s.db).await?;
    Ok(Json(json!({
        "enabled": true, "current_year": current_year(&st), "settings": st,
        "sources": sources.into_iter().map(|r| json!({"code": r.0, "folder_name": r.1, "group": r.2, "keep_versions": r.3, "path_prefixes": r.4})).collect::<Vec<_>>(),
        "stages": stages.into_iter().map(|r| json!({"fiscal_year": r.0, "source": r.1, "code": r.2, "name": r.3, "active": r.4})).collect::<Vec<_>>(),
    })))
}

/// PUT /api/layout/stages/:year — the year's stage list (replaces it); stage
/// folders already made are renamed to match.
pub async fn put_stages(State(s): State<Arc<AppState>>, admin: AdminUser, Path(year): Path<i32>, Json(list): Json<Vec<StageIn>>) -> ApiResult<Json<Value>> {
    let st = settings(&s.db).await.ok_or_else(|| ApiError::BadRequest("No basket has a folder layout.".into()))?;
    if !(2000..=3000).contains(&year) { return Err(ApiError::BadRequest("Fiscal year out of range.".into())); }
    let mut tx = s.db.begin().await?;
    sqlx::query("UPDATE layout_stages SET active = false WHERE system_id = $1 AND fiscal_year = $2").bind(&st.system_id).bind(year).execute(&mut *tx).await?;
    for (i, x) in list.iter().enumerate() {
        if x.code.trim().is_empty() || x.name.trim().is_empty() || x.source.trim().is_empty() {
            return Err(ApiError::BadRequest(format!("Row {}: source, code and name are required.", i + 1)));
        }
        sqlx::query(
            "INSERT INTO layout_stages (id, system_id, fiscal_year, source_code, code, name, sort, active) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT (system_id, fiscal_year, code) DO UPDATE SET source_code = EXCLUDED.source_code, name = EXCLUDED.name, sort = EXCLUDED.sort, active = EXCLUDED.active",
        ).bind(crate::new_id()).bind(&st.system_id).bind(year).bind(x.source.trim()).bind(x.code.trim()).bind(x.name.trim())
         .bind(if x.sort == 0 { i as i32 + 1 } else { x.sort }).bind(x.active).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    let renamed = rename_stages(&s.db, &st.system_id).await?;
    crate::basket::log_activity(&s.db, &admin.0, &format!("stage list {year} updated ({renamed} folders renamed)"), &year.to_string(), "folder", None, &st.system_id).await;
    Ok(Json(json!({ "stages": list.len(), "renamed": renamed })))
}

/// POST /api/layout/prepare — create the year folders now (the worker does it hourly).
pub async fn post_prepare(State(s): State<Arc<AppState>>, _: AdminUser) -> ApiResult<Json<Value>> {
    let n = prepare(&s.db).await?;
    Ok(Json(json!({ "years": n })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_without_org_or_source_is_empty() {
        assert!(Hint::default().is_empty());
        assert!(!Hint { source_code: Some("X".into()), ..Default::default() }.is_empty());
    }

    #[test]
    fn slashes_become_look_alikes() {
        assert_eq!(folder_name("2.1 a / b"), "2.1 a \u{FF0F} b");
        assert_eq!(folder_name(".."), "_");
    }

    #[test]
    fn configured_year_wins() {
        let st = Settings { system_id: "s".into(), current_year: Some(2569), fy_start_month: 10, year_offset: 543, main_name: "M".into(), support_name: "S".into() };
        assert_eq!(current_year(&st), 2569);
    }
}

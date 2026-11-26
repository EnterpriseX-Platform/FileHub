//! Basket settings (categories, fields, statuses, policy), status changes,
//! moving files/folders, and folder rename/re-parent.
//!
//! "Basket" is the UI name of a `systems` row. Everything here is data the
//! basket admin sets — nothing about any one business is hard-coded.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use crate::Id;

use crate::acl::{self, EDIT, MANAGE, VIEW};
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Category {
    #[serde(default)]
    pub id: String,
    pub parent_code: Option<String>,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub sort: i32,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct FieldDef {
    pub key: String,
    pub label: String,
    #[serde(default = "default_text")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "empty_arr")]
    pub choices: Value,
    #[serde(default = "yes")]
    pub searchable: bool,
    #[serde(default)]
    pub show_in_list: bool,
    #[serde(default)]
    pub sort: i32,
}
fn default_text() -> String { "text".into() }
fn empty_arr() -> Value { Value::Array(vec![]) }
fn yes() -> bool { true }

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct StatusDef {
    pub code: String,
    pub label: String,
    #[serde(default = "gray")]
    pub color: String,
    #[serde(default)]
    pub is_initial: bool,
    #[serde(default)]
    pub locks_content: bool,
    /// Files in this status show in "Awaiting my review" for people with Manage.
    #[serde(default)]
    pub awaits_review: bool,
    #[serde(default)]
    pub sort: i32,
}
fn gray() -> String { "gray".into() }

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Transition {
    pub from_code: String,
    pub to_code: String,
    #[serde(default = "two")]
    pub required_level: i16,
}
fn two() -> i16 { 2 }

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, Default)]
pub struct Policy {
    pub encrypt: Option<bool>,
    pub allowed_ext: Option<Vec<String>>,
    pub max_file_bytes: Option<i64>,
    pub public_links: Option<bool>,
    #[sqlx(default)]
    pub stamp_downloads: Option<bool>,
    /// Lock new files for this many days (object lock / WORM).
    #[sqlx(default)]
    pub lock_days: Option<i32>,
    #[sqlx(default)]
    pub lock_mode: Option<String>,
    /// Keep earlier versions when a new one arrives (None = inherit; default yes).
    #[sqlx(default)]
    pub keep_versions: Option<bool>,
    /// How files here are encrypted: "symmetric" (AES-256, the server key) or
    /// "asymmetric" (a data key per file wrapped with the server's RSA key).
    /// None = inherit; default symmetric.
    #[sqlx(default)]
    pub encrypt_mode: Option<String>,
    /// The person uploading may choose to encrypt a single file (None = inherit; default no).
    #[sqlx(default)]
    pub file_encrypt_choice: Option<bool>,
    /// Encryption that choice uses: "symmetric" (AES-256) or "asymmetric" (RSA). Default symmetric.
    #[sqlx(default)]
    pub file_encrypt_mode: Option<String>,
    /// Index the text inside files so search finds it (None = inherit; default on).
    #[sqlx(default)]
    pub content_search: Option<bool>,
}

impl Policy {
    /// Fill what this level left open from a level further up.
    fn inherit(&mut self, up: Policy) {
        self.encrypt = self.encrypt.or(up.encrypt);
        self.allowed_ext = self.allowed_ext.take().or(up.allowed_ext);
        self.max_file_bytes = self.max_file_bytes.or(up.max_file_bytes);
        self.public_links = self.public_links.or(up.public_links);
        self.stamp_downloads = self.stamp_downloads.or(up.stamp_downloads);
        self.lock_days = self.lock_days.or(up.lock_days);
        self.lock_mode = self.lock_mode.take().or(up.lock_mode);
        self.keep_versions = self.keep_versions.or(up.keep_versions);
        self.encrypt_mode = self.encrypt_mode.take().or(up.encrypt_mode);
        self.file_encrypt_choice = self.file_encrypt_choice.or(up.file_encrypt_choice);
        self.file_encrypt_mode = self.file_encrypt_mode.take().or(up.file_encrypt_mode);
        self.content_search = self.content_search.or(up.content_search);
    }

    /// Search looks inside files here.
    pub fn content_search_on(&self) -> bool { self.content_search != Some(false) }

    /// The person uploading may tick "encrypt this file".
    pub fn file_choice(&self) -> bool { self.file_encrypt_choice == Some(true) }

    /// That choice encrypts with the RSA key (format v3).
    pub fn file_choice_asymmetric(&self) -> bool { self.file_encrypt_mode.as_deref() == Some("asymmetric") }

    /// Files here are encrypted with the RSA key (format v3).
    pub fn asymmetric(&self) -> bool {
        self.encrypt_mode.as_deref() == Some("asymmetric")
    }

    /// Allowed extensions as stored: lowercase, no dots, no blanks.
    pub fn clean_ext(&self) -> Option<Vec<String>> {
        self.allowed_ext.as_ref().map(|v| v.iter().map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect())
    }

    /// The lock settings, checked: days ≥ 1 with a mode, or neither.
    fn check_lock(&self) -> Result<(), ApiError> {
        match (self.lock_days, self.lock_mode.as_deref()) {
            (None, None) | (Some(0), _) => Ok(()),
            (Some(d), Some("GOVERNANCE" | "COMPLIANCE")) if d > 0 && d <= 36_500 => Ok(()),
            _ => Err(bad("Lock new files needs a number of days (1–36500) and a mode (GOVERNANCE or COMPLIANCE).")),
        }
    }
}

/// Columns of `basket_policies` that make up a [`Policy`].
const POLICY_COLS: &str = "encrypt, allowed_ext, max_file_bytes, public_links, stamp_downloads, lock_days, lock_mode, keep_versions, encrypt_mode, file_encrypt_choice, file_encrypt_mode, content_search";

async fn load_policy<'e, E: sqlx::PgExecutor<'e>>(db: E, scope_type: &str, scope_id: &str) -> Result<Policy, ApiError> {
    Ok(sqlx::query_as(&format!("SELECT {POLICY_COLS} FROM basket_policies WHERE scope_type = $1 AND scope_id = $2"))
        .bind(scope_type).bind(scope_id).fetch_optional(db).await?.unwrap_or_default())
}

async fn save_policy<'e, E: sqlx::PgExecutor<'e>>(db: E, scope_type: &str, scope_id: &str, p: &Policy) -> Result<(), ApiError> {
    p.check_lock()?;
    if let Some(m) = p.encrypt_mode.as_deref() {
        if !["symmetric", "asymmetric"].contains(&m) { return Err(bad("Encryption must be symmetric or asymmetric.")); }
    }
    if let Some(m) = p.file_encrypt_mode.as_deref() {
        if !["symmetric", "asymmetric"].contains(&m) { return Err(bad("Encryption for single files must be symmetric or asymmetric.")); }
    }
    sqlx::query(&format!(
        "INSERT INTO basket_policies (scope_type, scope_id, {POLICY_COLS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
         ON CONFLICT (scope_type, scope_id) DO UPDATE SET
           encrypt = EXCLUDED.encrypt, allowed_ext = EXCLUDED.allowed_ext, max_file_bytes = EXCLUDED.max_file_bytes,
           public_links = EXCLUDED.public_links, stamp_downloads = EXCLUDED.stamp_downloads,
           lock_days = EXCLUDED.lock_days, lock_mode = EXCLUDED.lock_mode, keep_versions = EXCLUDED.keep_versions,
           encrypt_mode = EXCLUDED.encrypt_mode, file_encrypt_choice = EXCLUDED.file_encrypt_choice,
           file_encrypt_mode = EXCLUDED.file_encrypt_mode, content_search = EXCLUDED.content_search"))
        .bind(scope_type).bind(scope_id).bind(p.encrypt).bind(p.clean_ext()).bind(p.max_file_bytes).bind(p.public_links)
        .bind(p.stamp_downloads).bind(p.lock_days.filter(|d| *d > 0)).bind(&p.lock_mode).bind(p.keep_versions).bind(&p.encrypt_mode)
        .bind(p.file_encrypt_choice).bind(&p.file_encrypt_mode).bind(p.content_search)
        .execute(db).await?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    pub categories: Vec<Category>,
    pub fields: Vec<FieldDef>,
    pub statuses: Vec<StatusDef>,
    pub transitions: Vec<Transition>,
    pub policy: Policy,
}

pub async fn load_settings(db: &PgPool, sys: &str) -> Result<Settings, ApiError> {
    Ok(Settings {
        categories: sqlx::query_as("SELECT id, parent_code, code, name, sort FROM basket_categories WHERE system_id = $1 ORDER BY sort, name")
            .bind(sys).fetch_all(db).await?,
        fields: sqlx::query_as("SELECT key, label, field_type, required, choices, searchable, show_in_list, sort FROM basket_fields WHERE system_id = $1 ORDER BY sort, key")
            .bind(sys).fetch_all(db).await?,
        statuses: sqlx::query_as("SELECT code, label, color, is_initial, locks_content, awaits_review, sort FROM basket_statuses WHERE system_id = $1 ORDER BY sort, code")
            .bind(sys).fetch_all(db).await?,
        transitions: sqlx::query_as("SELECT from_code, to_code, required_level FROM basket_status_transitions WHERE system_id = $1 ORDER BY from_code, to_code")
            .bind(sys).fetch_all(db).await?,
        policy: load_policy(db, "basket", sys).await?,
    })
}

pub async fn get_settings(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(sys): Path<String>,
) -> ApiResult<Json<Settings>> {
    // Labels, columns and statuses are needed by anyone who can see any file
    // in the basket (e.g. through a folder grant), not only basket-wide viewers.
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM systems WHERE id = $1 AND deleted_at IS NULL").bind(&sys).fetch_optional(&s.db).await?;
    if exists.is_none() { return Err(ApiError::NotFound); }
    crate::auth::ensure_system_access(&s.db, &user.0, &sys).await.map_err(|_| ApiError::NotFound)?;
    Ok(Json(load_settings(&s.db, &sys).await?))
}

fn bad(m: impl Into<String>) -> ApiError { ApiError::BadRequest(m.into()) }

/// Replace a basket's settings in one transaction. Manage on the basket required.
pub async fn put_settings(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(sys): Path<String>,
    Json(b): Json<Settings>,
) -> ApiResult<Json<Settings>> {
    acl::require_folder(&s.db, &user.0, &sys, None, MANAGE).await?;

    // ---- validate -----------------------------------------------------------
    let code_ok = |c: &str| !c.is_empty() && c.len() <= 40 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    let mut seen = std::collections::HashSet::new();
    for c in &b.categories {
        if !code_ok(&c.code) { return Err(bad(format!("Category code \"{}\" may use letters, digits, - and _ only.", c.code))); }
        if c.name.trim().is_empty() { return Err(bad(format!("Category {} needs a name.", c.code))); }
        if !seen.insert(c.code.clone()) { return Err(bad(format!("Category code {} is used twice.", c.code))); }
    }
    for c in &b.categories {
        if let Some(p) = &c.parent_code { if !seen.contains(p) { return Err(bad(format!("Category {} has unknown parent {}.", c.code, p))); } }
    }
    let mut keys = std::collections::HashSet::new();
    for f in &b.fields {
        let ok = f.key.len() <= 40 && f.key.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
            && f.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !ok { return Err(bad(format!("Field key \"{}\" must start with a-z and use a-z, 0-9 and _ only.", f.key))); }
        if !["text", "number", "date", "choice", "user", "org"].contains(&f.field_type.as_str()) {
            return Err(bad(format!("Field {} has unknown type {}.", f.key, f.field_type)));
        }
        if f.field_type == "choice" && f.choices.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Err(bad(format!("Field {} is a choice field but has no choices.", f.key)));
        }
        if !keys.insert(f.key.clone()) { return Err(bad(format!("Field key {} is used twice.", f.key))); }
    }
    if b.statuses.is_empty() { return Err(bad("A basket needs at least one status.")); }
    if b.statuses.iter().filter(|x| x.is_initial).count() != 1 { return Err(bad("Mark exactly one status as the initial status.")); }
    let mut codes = std::collections::HashSet::new();
    for st in &b.statuses {
        if st.code.trim().is_empty() || st.label.trim().is_empty() { return Err(bad("Every status needs a code and a label.")); }
        if !["gray", "blue", "green", "orange", "red"].contains(&st.color.as_str()) { return Err(bad(format!("Status {} has unknown color {}.", st.code, st.color))); }
        if !codes.insert(st.code.clone()) { return Err(bad(format!("Status {} is listed twice.", st.code))); }
    }
    for t in &b.transitions {
        if !codes.contains(&t.from_code) || !codes.contains(&t.to_code) {
            return Err(bad(format!("Transition {} → {} uses a status that is not in the list.", t.from_code, t.to_code)));
        }
        if !(1..=3).contains(&t.required_level) { return Err(bad("Transition level must be 1 (view), 2 (edit) or 3 (manage).")); }
    }
    // A status still used by files cannot disappear.
    let in_use: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM files WHERE system_id = $1 AND deleted_at IS NULL GROUP BY status",
    ).bind(&sys).fetch_all(&s.db).await?;
    for (code, n) in in_use {
        if !codes.contains(&code) {
            return Err(bad(format!("{n} files still have status \"{code}\". Change their status before removing it.")));
        }
    }
    if let Some(m) = b.policy.max_file_bytes { if m < 0 { return Err(bad("Maximum file size cannot be negative.")); } }
    let mut b = b;
    if is_vault(&s.db, &sys).await? {
        // A Vault's protections are not settings.
        b.policy.encrypt = Some(true);
        b.policy.public_links = Some(false);
        b.policy.stamp_downloads = Some(true);
    }

    // ---- write --------------------------------------------------------------
    let mut tx = s.db.begin().await?;
    for t in ["basket_categories", "basket_fields", "basket_statuses", "basket_status_transitions"] {
        sqlx::query(&format!("DELETE FROM {t} WHERE system_id = $1")).bind(&sys).execute(&mut *tx).await?;
    }
    for (i, c) in b.categories.iter().enumerate() {
        sqlx::query("INSERT INTO basket_categories (id, system_id, parent_code, code, name, sort) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(crate::new_id()).bind(&sys).bind(&c.parent_code).bind(&c.code).bind(c.name.trim())
            .bind(if c.sort == 0 { i as i32 + 1 } else { c.sort }).execute(&mut *tx).await?;
    }
    for (i, f) in b.fields.iter().enumerate() {
        sqlx::query("INSERT INTO basket_fields (system_id, key, label, field_type, required, choices, searchable, show_in_list, sort) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(&sys).bind(&f.key).bind(f.label.trim()).bind(&f.field_type).bind(f.required).bind(&f.choices)
            .bind(f.searchable).bind(f.show_in_list).bind(if f.sort == 0 { i as i32 + 1 } else { f.sort })
            .execute(&mut *tx).await?;
    }
    for (i, st) in b.statuses.iter().enumerate() {
        sqlx::query("INSERT INTO basket_statuses (system_id, code, label, color, is_initial, locks_content, sort, awaits_review) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(&sys).bind(st.code.trim()).bind(st.label.trim()).bind(&st.color).bind(st.is_initial).bind(st.locks_content)
            .bind(if st.sort == 0 { i as i32 + 1 } else { st.sort }).bind(st.awaits_review).execute(&mut *tx).await?;
    }
    for t in &b.transitions {
        sqlx::query("INSERT INTO basket_status_transitions (system_id, from_code, to_code, required_level) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(&sys).bind(&t.from_code).bind(&t.to_code).bind(t.required_level).execute(&mut *tx).await?;
    }
    let was_on = load_policy(&mut *tx, "basket", &sys).await?.content_search_on();
    save_policy(&mut *tx, "basket", &sys, &b.policy).await?;
    tx.commit().await?;
    // Content search switched: drop the basket's text index, or build it again.
    let now_on = b.policy.content_search_on();
    if was_on && !now_on {
        sqlx::query("DELETE FROM file_content WHERE file_id IN (SELECT id FROM files WHERE system_id = $1)").bind(&sys).execute(&s.db).await?;
    } else if !was_on && now_on {
        let rows: Vec<(Id, String, String, String, bool, i64)> = sqlx::query_as(
            "SELECT id, file_type, name, object_key, encrypted, size_bytes FROM files WHERE system_id = $1 AND deleted_at IS NULL")
            .bind(&sys).fetch_all(&s.db).await?;
        for (id, ft, name, key, enc, size) in rows {
            crate::handlers::spawn_p1(&s, id, ft, name, key, enc, size.max(0) as u64);
        }
    }

    log_activity(&s.db, &user.0, "settings changed", &sys, "basket", None, &sys).await;
    Ok(Json(load_settings(&s.db, &sys).await?))
}

// -----------------------------------------------------------------------------
// Field values + policy checks used by upload / patch
// -----------------------------------------------------------------------------

/// Check `values` against the basket's field definitions. Unknown keys are kept
/// (integrations may send extra metadata); defined keys are type-checked and
/// required ones must be present when `enforce_required`.
pub async fn validate_fields(db: &PgPool, sys: &str, values: &Value, enforce_required: bool) -> Result<Value, ApiError> {
    let obj = match values {
        Value::Null => serde_json::Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(bad("Fields must be a JSON object.")),
    };
    let defs: Vec<FieldDef> = sqlx::query_as("SELECT key, label, field_type, required, choices, searchable, show_in_list, sort FROM basket_fields WHERE system_id = $1")
        .bind(sys).fetch_all(db).await?;
    for d in &defs {
        let v = obj.get(&d.key);
        let empty = matches!(v, None | Some(Value::Null)) || v.and_then(|x| x.as_str()).map(|x| x.trim().is_empty()).unwrap_or(false);
        if empty {
            if enforce_required && d.required { return Err(bad(format!("{} is required.", d.label))); }
            continue;
        }
        let v = v.unwrap();
        let ok = match d.field_type.as_str() {
            "number" => v.is_number() || v.as_str().map(|x| x.trim().parse::<f64>().is_ok()).unwrap_or(false),
            "date" => v.as_str().map(|x| chrono::NaiveDate::parse_from_str(x.get(..10).unwrap_or(x), "%Y-%m-%d").is_ok()).unwrap_or(false),
            "choice" => d.choices.as_array().map(|c| c.iter().any(|o| o == v || o.get("value") == Some(v))).unwrap_or(false),
            _ => v.is_string() || v.is_number(),
        };
        if !ok { return Err(bad(format!("{} has a value this basket does not accept.", d.label))); }
    }
    Ok(Value::Object(obj))
}

pub async fn check_category(db: &PgPool, sys: &str, code: Option<&str>) -> Result<(), ApiError> {
    let Some(code) = code.filter(|c| !c.is_empty()) else { return Ok(()) };
    let ok: Option<(String,)> = sqlx::query_as("SELECT code FROM basket_categories WHERE system_id = $1 AND code = $2")
        .bind(sys).bind(code).fetch_optional(db).await?;
    ok.map(|_| ()).ok_or_else(|| bad(format!("Category {code} does not exist in this basket.")))
}

pub async fn initial_status(db: &PgPool, sys: &str) -> Result<Option<String>, ApiError> {
    Ok(sqlx::query_scalar("SELECT code FROM basket_statuses WHERE system_id = $1 AND is_initial LIMIT 1")
        .bind(sys).fetch_optional(db).await?)
}

/// The policy that applies in a folder: the folder's own settings, then each
/// parent's, then the basket's — the nearest level wins. One query.
pub async fn effective_policy(db: &PgPool, sys: &str, folder: Option<&str>) -> Result<Policy, ApiError> {
    let cols = POLICY_COLS.split(", ").map(|c| format!("p.{c}")).collect::<Vec<_>>().join(", ");
    let levels: Vec<Policy> = sqlx::query_as(&format!(
        r#"WITH RECURSIVE up(id, parent_id, depth) AS (
              SELECT id, parent_id, 0 FROM folders WHERE id = $2
              UNION ALL
              SELECT f.id, f.parent_id, up.depth + 1 FROM folders f JOIN up ON f.id = up.parent_id WHERE up.depth < 64)
           SELECT {cols}
             FROM (SELECT 'folder' AS t, id AS sid, depth FROM up
                   UNION ALL SELECT 'basket', $1, 1000) lv
             JOIN basket_policies p ON p.scope_type = lv.t AND p.scope_id = lv.sid
            ORDER BY lv.depth"#
    )).bind(sys).bind(folder).fetch_all(db).await?;
    let mut p = Policy::default();
    for level in levels { p.inherit(level); }
    Ok(p)
}

/// Fill `effective_encrypted` on folders: the nearest folder (self, then
/// parents) with an encryption setting wins, else the basket's. One query.
pub async fn fill_effective_encrypted(db: &PgPool, folders: &mut [crate::models::Folder]) -> Result<(), ApiError> {
    if folders.is_empty() { return Ok(()); }
    let ids: Vec<String> = folders.iter().map(|f| f.id.clone()).collect();
    let rows: Vec<(String, bool)> = sqlx::query_as(
        r#"WITH RECURSIVE up(start, id, parent_id, depth) AS (
              SELECT id, id, parent_id, 0 FROM folders WHERE id = ANY($1)
              UNION ALL
              SELECT up.start, f.id, f.parent_id, up.depth + 1 FROM folders f JOIN up ON f.id = up.parent_id WHERE up.depth < 64)
           SELECT s.id, COALESCE(
                (SELECT p.encrypt FROM up JOIN basket_policies p ON p.scope_type = 'folder' AND p.scope_id = up.id
                  WHERE up.start = s.id AND p.encrypt IS NOT NULL ORDER BY up.depth LIMIT 1),
                (SELECT p.encrypt FROM basket_policies p WHERE p.scope_type = 'basket' AND p.scope_id = s.system_id),
                false)
             FROM folders s WHERE s.id = ANY($1)"#,
    ).bind(&ids).fetch_all(db).await?;
    let m: std::collections::HashMap<String, bool> = rows.into_iter().collect();
    for f in folders.iter_mut() { f.effective_encrypted = f.encrypted || m.get(&f.id).copied().unwrap_or(false); }
    Ok(())
}

/// Refuse a file the destination's policy does not accept.
pub async fn check_policy(db: &PgPool, sys: &str, folder: Option<&str>, name: &str, size: i64) -> Result<Policy, ApiError> {
    let p = effective_policy(db, sys, folder).await?;
    let ext = std::path::Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if let Some(list) = &p.allowed_ext {
        if !list.is_empty() && !list.iter().any(|e| e == &ext) {
            return Err(bad(format!(".{} files aren't allowed here. Allowed: {}.", if ext.is_empty() { "(no extension)" } else { &ext }, list.join(", "))));
        }
    }
    if let Some(max) = p.max_file_bytes {
        if max > 0 && size > max {
            return Err(ApiError::PayloadTooLarge(format!("This file is larger than the {} limit here.", human_bytes(max))));
        }
    }
    Ok(p)
}

/// Largest file a person may upload where no folder/basket policy says
/// otherwise (`DEFAULT_MAX_FILE_MB`, default 20; 0 = no limit). A policy's own
/// max_file_bytes (0 = no limit) always wins.
pub fn default_max_file_bytes() -> i64 {
    std::env::var("DEFAULT_MAX_FILE_MB").ok().and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(20).max(0) * 1024 * 1024
}

/// [`check_policy`] for uploads people make (web, chunked, new versions):
/// also applies the default size limit when no policy sets one.
pub async fn check_upload_policy(db: &PgPool, sys: &str, folder: Option<&str>, name: &str, size: i64) -> Result<Policy, ApiError> {
    let p = check_policy(db, sys, folder, name, size).await?;
    if p.max_file_bytes.is_none() {
        let max = default_max_file_bytes();
        if max > 0 && size > max {
            return Err(ApiError::Coded(axum::http::StatusCode::PAYLOAD_TOO_LARGE, "too_large",
                format!("\"{name}\" is {}, which exceeds the {} upload limit.", human_bytes(size), human_bytes(max))));
        }
    }
    Ok(p)
}

pub fn human_bytes(b: i64) -> String {
    let f = b as f64;
    if f >= 1024.0 * 1024.0 * 1024.0 { format!("{:.1} GB", f / 1073741824.0) }
    else if f >= 1024.0 * 1024.0 { format!("{:.0} MB", f / 1048576.0) }
    else { format!("{:.0} KB", (f / 1024.0).max(1.0)) }
}

/// A folder name that can't be used to climb out of a folder when a zip of it
/// is extracted: 1–200 characters, no slashes, not "." or "..".
pub fn valid_folder_name(n: &str) -> bool {
    !n.is_empty() && n.chars().count() <= 200 && !n.contains(['/', '\\']) && n != "." && n != ".."
}

/// True when the file's current status locks its content (no new versions, no rename).
pub async fn content_locked(db: &PgPool, sys: &str, status: &str) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar::<_, bool>("SELECT locks_content FROM basket_statuses WHERE system_id = $1 AND code = $2")
        .bind(sys).bind(status).fetch_optional(db).await?.unwrap_or(false))
}

// -----------------------------------------------------------------------------
// Status change
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StatusChange { pub to: String, pub comment: Option<String> }

#[derive(Debug, Serialize)]
pub struct NextStatus { pub code: String, pub label: String, pub color: String }

/// Statuses the caller may move this file to now.
pub async fn next_statuses(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
) -> ApiResult<Json<Vec<NextStatus>>> {
    let lvl = acl::require_file(&s.db, &user.0, id, VIEW).await?;
    let (sys, status): (String, String) = sqlx::query_as("SELECT system_id, status FROM files WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT st.code, st.label, st.color FROM basket_status_transitions t
             JOIN basket_statuses st ON st.system_id = t.system_id AND st.code = t.to_code
            WHERE t.system_id = $1 AND t.from_code = $2 AND t.required_level <= $3
            ORDER BY st.sort"#,
    ).bind(&sys).bind(&status).bind(lvl as i16).fetch_all(&s.db).await?;
    Ok(Json(rows.into_iter().map(|(code, label, color)| NextStatus { code, label, color }).collect()))
}

pub async fn change_status(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
    Json(b): Json<StatusChange>,
) -> ApiResult<Json<Value>> {
    let lvl = acl::require_file(&s.db, &user.0, id, VIEW).await?;
    let (sys, status, name): (String, String, String) =
        sqlx::query_as("SELECT system_id, status, name FROM files WHERE id = $1 AND deleted_at IS NULL")
            .bind(id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    if b.to == status { return Err(bad("The file already has this status.")); }
    let need: Option<i16> = sqlx::query_scalar(
        "SELECT required_level FROM basket_status_transitions WHERE system_id = $1 AND from_code = $2 AND to_code = $3",
    ).bind(&sys).bind(&status).bind(&b.to).fetch_optional(&s.db).await?;
    let Some(need) = need else {
        return Err(bad(format!("This basket doesn't allow moving from \"{status}\" to \"{}\".", b.to)));
    };
    if (lvl as i16) < need { return Err(ApiError::Forbidden); }
    sqlx::query("UPDATE files SET status = $1, modified_at = now() WHERE id = $2").bind(&b.to).bind(id).execute(&s.db).await?;
    let label: String = sqlx::query_scalar("SELECT label FROM basket_statuses WHERE system_id=$1 AND code=$2")
        .bind(&sys).bind(&b.to).fetch_one(&s.db).await?;
    log_activity(&s.db, &user.0, &format!("status → {label}"), &name, "file", Some(id), &sys).await;
    crate::webhooks::emit(&s.db, "file.status", &sys, serde_json::json!({ "id": id, "name": name, "status": b.to, "label": label })).await;
    if let Some(c) = b.comment.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        let _ = sqlx::query(
            "INSERT INTO file_comments (id, file_id, user_id, body, created_at) VALUES ($1,$2,$3,$4,now())",
        ).bind(crate::Id::new()).bind(id).bind(&user.0.id).bind(c).execute(&s.db).await;
    }
    Ok(Json(serde_json::json!({ "id": id, "status": b.to, "label": label })))
}

// -----------------------------------------------------------------------------
// Folder rename / re-parent / inherit
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PatchFolder {
    pub name: Option<String>,
    pub color: Option<String>,
}

pub async fn patch_folder(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(b): Json<PatchFolder>,
) -> ApiResult<Json<crate::models::Folder>> {
    let (sys, parent): (String, Option<String>) = sqlx::query_as("SELECT system_id, parent_id FROM folders WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    acl::require_folder(&s.db, &user.0, &sys, Some(&id), EDIT).await?;
    if let Some(n) = b.name.as_deref().map(str::trim) {
        if !valid_folder_name(n) { return Err(bad("Folder name must be 1–200 characters, without / or \\, and not . or ..")); }
        if folder_name_taken(&s.db, &sys, parent.as_deref(), n, Some(&id)).await? {
            return Err(bad(format!("A folder named \"{n}\" is already here.")));
        }
        sqlx::query("UPDATE folders SET name = $1 WHERE id = $2").bind(n).bind(&id).execute(&s.db).await?;
    }
    if let Some(c) = b.color { sqlx::query("UPDATE folders SET color = $1 WHERE id = $2").bind(c).bind(&id).execute(&s.db).await?; }
    Ok(Json(sqlx::query_as("SELECT * FROM folders WHERE id = $1").bind(&id).fetch_one(&s.db).await?))
}

// -----------------------------------------------------------------------------
// Move files and folders (TOR 4.15.12)
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MoveReq {
    #[serde(default)]
    pub file_ids: Vec<Id>,
    #[serde(default)]
    pub folder_ids: Vec<String>,
    pub target_system_id: String,
    /// None = basket root.
    pub target_folder_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MoveResult { pub moved_files: usize, pub moved_folders: usize, pub target: String }

pub async fn move_items(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(b): Json<MoveReq>,
) -> ApiResult<Json<MoveResult>> {
    if b.file_ids.is_empty() && b.folder_ids.is_empty() { return Err(bad("Nothing selected to move.")); }
    let tsys = b.target_system_id.clone();
    let tfld = b.target_folder_id.clone().filter(|f| !f.is_empty());
    if let Some(f) = &tfld {
        let fs: Option<(String,)> = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL")
            .bind(f).fetch_optional(&s.db).await?;
        match fs { Some((x,)) if x == tsys => {}, _ => return Err(bad("The destination folder is not in the destination basket.")) }
    }
    acl::require_folder(&s.db, &user.0, &tsys, tfld.as_deref(), EDIT).await?;
    let target_bucket: String = sqlx::query_scalar("SELECT bucket FROM systems WHERE id = $1 AND deleted_at IS NULL AND status <> 'paused'")
        .bind(&tsys).fetch_optional(&s.db).await?.ok_or_else(|| bad("The destination basket doesn't accept files."))?;

    // Folders: no cycles; collect each subtree.
    let mut moved_folders = 0;
    for fid in &b.folder_ids {
        let (fsys,): (String,) = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL")
            .bind(fid).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
        acl::require_folder(&s.db, &user.0, &fsys, Some(fid), EDIT).await?;
        let subtree: Vec<String> = sqlx::query_scalar(
            r#"WITH RECURSIVE t AS (SELECT id FROM folders WHERE id = $1
                 UNION ALL SELECT f.id FROM folders f JOIN t ON f.parent_id = t.id WHERE f.deleted_at IS NULL)
               SELECT id FROM t"#,
        ).bind(fid).fetch_all(&s.db).await?;
        if let Some(t) = &tfld { if subtree.contains(t) { return Err(bad("A folder can't be moved into itself or one of its subfolders.")); } }
        let mut tx = s.db.begin().await?;
        sqlx::query("UPDATE folders SET parent_id = $1 WHERE id = $2").bind(&tfld).bind(fid).execute(&mut *tx).await?;
        if fsys != tsys {
            sqlx::query("UPDATE folders SET system_id = $1 WHERE id = ANY($2)").bind(&tsys).bind(&subtree).execute(&mut *tx).await?;
            // Grants belong to the basket they were made in.
            sqlx::query("UPDATE acl_entries SET system_id = $1 WHERE resource_type='folder' AND resource_id = ANY($2)").bind(&tsys).bind(&subtree).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        if fsys != tsys {
            let files: Vec<Id> = sqlx::query_scalar("SELECT id FROM files WHERE folder_id = ANY($1) AND deleted_at IS NULL")
                .bind(&subtree).fetch_all(&s.db).await?;
            for id in files { relocate_file(&s, id, &tsys, &target_bucket, None).await?; }
        }
        moved_folders += 1;
    }

    let mut moved_files = 0;
    for id in &b.file_ids {
        acl::require_file(&s.db, &user.0, *id, EDIT).await?;
        let (name, size): (String, i64) = sqlx::query_as("SELECT name, size_bytes FROM files WHERE id = $1").bind(id).fetch_one(&s.db).await?;
        check_policy(&s.db, &tsys, tfld.as_deref(), &name, size).await?;
        relocate_file(&s, *id, &tsys, &target_bucket, Some(tfld.clone())).await?;
        if effective_policy(&s.db, &tsys, tfld.as_deref()).await?.encrypt == Some(true) && s.storage.has_key() {
            encrypt_file_now(&s, *id).await?;
        }
        moved_files += 1;
    }

    let target: String = match &tfld {
        Some(f) => sqlx::query_scalar("SELECT name FROM folders WHERE id = $1").bind(f).fetch_one(&s.db).await?,
        None => sqlx::query_scalar("SELECT name FROM systems WHERE id = $1").bind(&tsys).fetch_one(&s.db).await?,
    };
    log_activity(&s.db, &user.0, &format!("moved {moved_files} files, {moved_folders} folders"), &target, "folder", None, &tsys).await;
    Ok(Json(MoveResult { moved_files, moved_folders, target }))
}

/// Move one file to (basket, folder). `folder = None` keeps the folder id
/// (used when a whole folder subtree changes basket).
async fn relocate_file(s: &Arc<AppState>, id: Id, tsys: &str, tbucket: &str, folder: Option<Option<String>>) -> Result<(), ApiError> {
    let (sys, key, size, org): (String, String, i64, Option<String>) =
        sqlx::query_as("SELECT system_id, object_key, size_bytes, org_id FROM files WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    let mut new_key = key.clone();
    if sys != tsys {
        crate::handlers::enforce_quota(s, size, Some(tsys), org.as_deref(), None).await?;
        let tail = key.split_once('/').map(|(_, t)| t.to_string()).unwrap_or(key.clone());
        new_key = format!("{tbucket}/{tail}");
        if new_key != key { s.storage.rename(&key, &new_key).await?; }
        // Category / status codes may not exist in the new basket.
        sqlx::query(
            r#"UPDATE files f SET
                 category_code = CASE WHEN EXISTS (SELECT 1 FROM basket_categories c WHERE c.system_id = $1 AND c.code = f.category_code) THEN f.category_code END,
                 status = CASE WHEN EXISTS (SELECT 1 FROM basket_statuses st WHERE st.system_id = $1 AND st.code = f.status) THEN f.status
                               ELSE COALESCE((SELECT code FROM basket_statuses WHERE system_id = $1 AND is_initial LIMIT 1), f.status) END
               WHERE id = $2"#,
        ).bind(tsys).bind(id).execute(&s.db).await?;
        sqlx::query("UPDATE acl_entries SET system_id = $1 WHERE resource_type='file' AND resource_id = $2").bind(tsys).bind(id.to_string()).execute(&s.db).await?;
    }
    match folder {
        Some(f) => sqlx::query("UPDATE files SET system_id=$1, bucket=$2, object_key=$3, folder_id=$4, modified_at=now() WHERE id=$5")
            .bind(tsys).bind(tbucket).bind(&new_key).bind(f).bind(id).execute(&s.db).await?,
        None => sqlx::query("UPDATE files SET system_id=$1, bucket=$2, object_key=$3, modified_at=now() WHERE id=$4")
            .bind(tsys).bind(tbucket).bind(&new_key).bind(id).execute(&s.db).await?,
    };
    Ok(())
}

/// Add a row to the activity log. `sys` = "" for workspace-wide actions.
pub async fn log_activity(db: &PgPool, user: &crate::auth::User, action: &str, target: &str, ttype: &str, file: Option<Id>, sys: &str) {
    let _ = sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, created_at, actor_id)
           VALUES ($1,$2,'slate',$3,$4,$5,$6,NULLIF($7, ''),now(),$8)"#,
    ).bind(crate::Id::new()).bind(&user.display_name).bind(action).bind(target).bind(ttype).bind(file).bind(sys).bind(&user.id)
    .execute(db).await;
}

/// Default status set for a new basket (same as migration 0018 gives existing ones).
pub async fn seed_defaults(db: &PgPool, sys: &str) -> Result<(), ApiError> {
    for (code, label, color, init, locks, sort) in [
        ("Draft", "Draft", "gray", true, false, 1), ("Review", "In review", "orange", false, true, 2),
        ("Approved", "Approved", "green", false, true, 3), ("Archived", "Archived", "gray", false, true, 4),
    ] {
        sqlx::query("INSERT INTO basket_statuses (system_id, code, label, color, is_initial, locks_content, sort, awaits_review) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING")
            .bind(sys).bind(code).bind(label).bind(color).bind(init).bind(locks).bind(sort).bind(code == "Review").execute(db).await?;
    }
    for (f, t, l) in [("Draft", "Review", 2i16), ("Review", "Approved", 3), ("Review", "Draft", 3), ("Approved", "Archived", 3), ("Archived", "Draft", 3), ("Draft", "Archived", 2)] {
        sqlx::query("INSERT INTO basket_status_transitions (system_id, from_code, to_code, required_level) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(sys).bind(f).bind(t).bind(l).execute(db).await?;
    }
    Ok(())
}

pub async fn folder_detail(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let f: crate::models::Folder = sqlx::query_as("SELECT * FROM folders WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    acl::require_folder(&s.db, &user.0, &f.system_id, Some(&id), VIEW).await.map_err(|_| ApiError::NotFound)?;
    // Totals over the folder and everything below it.
    let (files, bytes, folders): (i64, i64, i64) = sqlx::query_as(
        r#"WITH RECURSIVE t AS (SELECT id FROM folders WHERE id = $1
                                UNION ALL SELECT c.id FROM folders c JOIN t ON c.parent_id = t.id WHERE c.deleted_at IS NULL)
           SELECT (SELECT COUNT(*) FROM files WHERE folder_id IN (SELECT id FROM t) AND deleted_at IS NULL)::bigint,
                  (SELECT COALESCE(SUM(size_bytes),0) FROM files WHERE folder_id IN (SELECT id FROM t) AND deleted_at IS NULL)::bigint,
                  (SELECT COUNT(*) - 1 FROM t)::bigint"#,
    ).bind(&id).fetch_one(&s.db).await?;
    let creator: Option<(String, String)> = match &f.created_by {
        Some(u) => sqlx::query_as("SELECT display_name, email FROM users WHERE id = $1").bind(u).fetch_optional(&s.db).await?,
        None => None,
    };
    let mut v = serde_json::to_value(&f).map_err(|e| ApiError::Other(e.into()))?;
    if let Some(o) = v.as_object_mut() {
        o.insert("file_count".into(), files.into());
        o.insert("total_bytes".into(), bytes.into());
        o.insert("subfolder_count".into(), folders.into());
        o.insert("creator_name".into(), creator.as_ref().map(|c| c.0.clone()).unwrap_or_else(|| f.owner.clone()).into());
        o.insert("creator_email".into(), creator.map(|c| c.1).into());
    }
    Ok(Json(v))
}

// -----------------------------------------------------------------------------
// Folder policy + encrypt one file (TOR 4.15.5)
// -----------------------------------------------------------------------------

pub async fn get_folder_policy(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let (sys,): (String,) = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL").bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    acl::require_folder(&s.db, &user.0, &sys, Some(&id), VIEW).await?;
    let own = load_policy(&s.db, "folder", &id).await?;
    let eff = effective_policy(&s.db, &sys, Some(&id)).await?;
    Ok(Json(serde_json::json!({ "own": own, "effective": eff, "server_has_key": s.storage.has_key(), "server_has_rsa_key": s.storage.has_rsa_key() })))
}

/// Set a folder's own policy. Turning encryption on encrypts the files already
/// inside (and below) the folder.
pub async fn put_folder_policy(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>, Json(mut p): Json<Policy>) -> ApiResult<Json<Value>> {
    let (sys,): (String,) = sqlx::query_as("SELECT system_id FROM folders WHERE id = $1 AND deleted_at IS NULL").bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    acl::require_folder(&s.db, &user.0, &sys, Some(&id), MANAGE).await?;
    if is_vault(&s.db, &sys).await? {
        if p.encrypt == Some(false) || p.public_links == Some(true) { return Err(bad("Folders in a Vault are always encrypted and can't have public links.")); }
        p.encrypt = Some(true); p.public_links = Some(false);
    }
    if p.encrypt == Some(true) && !s.storage.has_key() {
        return Err(bad("No encryption key is set up on the server (STORAGE_ENC_KEY), so this folder can't require encryption."));
    }
    if p.asymmetric() && !s.storage.has_rsa_key() {
        return Err(bad("No RSA key is set up on the server (STORAGE_RSA_PRIVATE_KEY_FILE), so this folder can't use asymmetric encryption."));
    }
    save_policy(&s.db, "folder", &id, &p).await?;
    let mut encrypted_now = 0;
    if p.encrypt == Some(true) {
        let files: Vec<Id> = sqlx::query_scalar(
            r#"WITH RECURSIVE t AS (SELECT id FROM folders WHERE id = $1 UNION ALL SELECT f.id FROM folders f JOIN t ON f.parent_id = t.id WHERE f.deleted_at IS NULL)
               SELECT id FROM files WHERE folder_id IN (SELECT id FROM t) AND deleted_at IS NULL AND NOT encrypted"#,
        ).bind(&id).fetch_all(&s.db).await?;
        for f in files { encrypt_file_now(&s, f).await?; encrypted_now += 1; }
    }
    log_activity(&s.db, &user.0, "folder policy changed", &id, "folder", None, &sys).await;
    Ok(Json(serde_json::json!({ "encrypted_now": encrypted_now })))
}

/// Encrypt one stored file in place (all its versions too).
/// People with Manage may always do it (AES). Where the basket/folder policy
/// lets people choose ("file_encrypt_choice"), anyone who can edit the file may,
/// with the kind of encryption that policy picks (AES or RSA).
pub async fn encrypt_file(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    let (sys, folder): (String, Option<String>) = sqlx::query_as("SELECT system_id, folder_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    let pol = effective_policy(&s.db, &sys, folder.as_deref()).await?;
    let asym = if acl::require_file(&s.db, &user.0, id, MANAGE).await.is_ok() {
        pol.file_choice() && pol.file_choice_asymmetric()
    } else {
        acl::require_file(&s.db, &user.0, id, acl::EDIT).await?;
        crate::handlers::check_file_choice(&s, &pol)?;
        pol.file_choice_asymmetric()
    };
    if asym && !s.storage.has_rsa_key() { return Err(bad("No RSA key is set up on the server.")); }
    if !asym && !s.storage.has_key() { return Err(bad("No encryption key is set up on the server (STORAGE_ENC_KEY).")); }
    let done = encrypt_file_now_mode(&s, id, asym).await?;
    let (sys, name): (String, String) = sqlx::query_as("SELECT system_id, name FROM files WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    if done { log_activity(&s.db, &user.0, "encrypted", &name, "file", Some(id), &sys).await; }
    Ok(Json(serde_json::json!({ "encrypted": true, "changed": done })))
}

async fn encrypt_file_now(s: &Arc<AppState>, id: Id) -> Result<bool, ApiError> {
    encrypt_file_now_mode(s, id, false).await
}

async fn encrypt_file_now_mode(s: &Arc<AppState>, id: Id, asym: bool) -> Result<bool, ApiError> {
    let (key, enc): (String, bool) = sqlx::query_as("SELECT object_key, encrypted FROM files WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    if enc { return Ok(false); }
    if crate::storage::is_mount(&key) { return Err(bad("Files in a folder on the server disk stay readable by other programs and can't be encrypted.")); }
    encrypt_object_mode(s, &key, asym).await?.then_some(()).ok_or(ApiError::NotFound)?;
    sqlx::query("UPDATE files SET encrypted = true WHERE id = $1").bind(id).execute(&s.db).await?;
    // Older versions are read with the file's flag, so they must be encrypted too.
    let vers: Vec<(String,)> = sqlx::query_as("SELECT object_key FROM file_versions WHERE file_id = $1 AND object_key <> $2").bind(id).bind(&key).fetch_all(&s.db).await?;
    for (vk,) in vers { encrypt_object_mode(s, &vk, asym).await?; }
    Ok(true)
}

/// Re-write one stored object encrypted, streamed (memory stays O(chunk)).
/// `false` when the object isn't there.
async fn encrypt_object_mode(s: &AppState, key: &str, asym: bool) -> Result<bool, ApiError> {
    let Some(plain) = s.storage.open_stream(key, false).await? else { return Ok(false) };
    let staged = s.storage.stage_stream(plain).await?;
    s.storage.commit_mode(staged, key, true, asym).await?;
    Ok(true)
}


pub async fn is_vault(db: &PgPool, sys: &str) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar::<_, bool>("SELECT vault FROM systems WHERE id = $1").bind(sys).fetch_optional(db).await?.unwrap_or(false))
}

/// Every read of a Vault file is recorded (who, what, when).
pub async fn log_vault_access(db: &PgPool, user: &crate::auth::User, sys: &str, file: Id, name: &str, what: &str) {
    if matches!(is_vault(db, sys).await, Ok(true)) {
        log_activity(db, user, what, name, "file", Some(file), sys).await;
    }
}

/// Another live folder with the same name (ignoring case) in the same place.
pub async fn folder_name_taken(db: &PgPool, system_id: &str, parent: Option<&str>, name: &str, except: Option<&str>) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM folders WHERE system_id = $1 AND parent_id IS NOT DISTINCT FROM $2
            AND lower(name) = lower($3) AND deleted_at IS NULL AND id IS DISTINCT FROM $4",
    ).bind(system_id).bind(parent).bind(name.trim()).bind(except).fetch_one(db).await?;
    Ok(n > 0)
}

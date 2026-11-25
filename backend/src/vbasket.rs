//! Virtual Baskets — saved rules over files that live in real baskets — and the
//! per-user download list (zip of files from many places).
//!
//! Rules compile to SQL with bound parameters only; field names come from a
//! fixed system list or a validated basket field key, never from raw input.

use std::collections::{BTreeMap, HashSet};
use std::io::Write as _;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use crate::Id;

use crate::acl;
use crate::auth::{AuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::handlers::FILE_COLS;
use crate::models::File;
use crate::state::AppState;

fn bad(m: impl Into<String>) -> ApiError { ApiError::BadRequest(m.into()) }

// -----------------------------------------------------------------------------
// Model
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Rule {
    Group { #[serde(rename = "match")] mode: String, rules: Vec<Rule> },
    Cond { field: String, op: String, #[serde(default)] value: Value },
}

#[derive(Debug, Serialize, FromRow)]
struct VRow {
    id: String,
    name: String,
    description: Option<String>,
    match_mode: String,
    rules: Value,
    group_by: Vec<String>,
    all_sources: bool,
    drop_target_system_id: Option<String>,
    drop_target_folder_id: Option<String>,
    columns: Vec<String>,
    personal: bool,
    shared_with: Vec<String>,
    created_by: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct VBasket {
    #[serde(flatten)]
    row: VRow,
    sources: Vec<String>,
    source_names: Vec<String>,
    can_edit: bool,
    drop_target_label: Option<String>,
    group_by_labels: Vec<String>,
    /// Field values an upload through the drop target gets so it matches the rules.
    prefill: Value,
}

#[derive(Debug, Deserialize)]
pub struct VBasketIn {
    pub name: String,
    pub description: Option<String>,
    #[serde(default = "all")]
    pub match_mode: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub all_sources: bool,
    pub drop_target_system_id: Option<String>,
    pub drop_target_folder_id: Option<String>,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub personal: bool,
    #[serde(default)]
    pub shared_with: Vec<String>,
}
fn all() -> String { "all".into() }

// -----------------------------------------------------------------------------
// Rule → SQL
// -----------------------------------------------------------------------------

enum Bind { S(String), A(Vec<String>) }

struct Sql { text: String, binds: Vec<Bind> }
impl Sql {
    fn new() -> Self { Sql { text: String::new(), binds: Vec::new() } }
    fn s(&mut self, v: impl Into<String>) -> String { self.binds.push(Bind::S(v.into())); format!("${}", self.binds.len()) }
    fn a(&mut self, v: Vec<String>) -> String { self.binds.push(Bind::A(v)); format!("${}", self.binds.len()) }
}

const SYS_FIELDS: &[(&str, &str, &str)] = &[
    ("sys.format", "Format", "choice"),
    ("sys.ext", "Extension", "text"),
    ("sys.category", "Category", "category"),
    ("sys.status", "Status", "status"),
    ("sys.basket", "Basket", "basket"),
    ("sys.org", "Uploader's agency", "org"),
    ("sys.name", "Name", "text"),
    ("sys.tag", "Tag", "text"),
    ("sys.created_at", "Uploaded", "date"),
    ("sys.modified_at", "Modified", "date"),
    ("sys.created_by", "Uploaded by", "user"),
    ("sys.size", "Size (bytes)", "number"),
    ("sys.month", "Month uploaded", "text"),
    ("sys.year", "Year uploaded", "number"),
];
pub const FORMATS: &[&str] = &["PDF", "Word", "Excel", "PowerPoint", "Image", "Audio", "Video", "Text", "Archive", "Other"];

fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 40 && k.chars().next().unwrap().is_ascii_lowercase()
        && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// SQL expression for a field (files alias `f`) and whether it is a timestamp / number.
fn field_expr(field: &str) -> Result<(String, &'static str), ApiError> {
    Ok(match field {
        "sys.format" => ("fh_format(f.name)".into(), "text"),
        "sys.ext" => ("fh_ext(f.name)".into(), "text"),
        "sys.category" => ("f.category_code".into(), "text"),
        "sys.status" => ("f.status".into(), "text"),
        "sys.basket" => ("f.system_id".into(), "text"),
        "sys.org" => ("f.org_id".into(), "text"),
        "sys.name" => ("f.name".into(), "text"),
        "sys.tag" => ("f.tags".into(), "tags"),
        "sys.created_at" => ("f.created_at".into(), "ts"),
        "sys.modified_at" => ("f.modified_at".into(), "ts"),
        "sys.created_by" => ("f.created_by".into(), "text"),
        "sys.size" => ("f.size_bytes".into(), "num"),
        "sys.month" => ("to_char(f.created_at, 'YYYY-MM')".into(), "text"),
        "sys.year" => ("extract(year from f.created_at)::int::text".into(), "text"),
        k if valid_key(k) => (format!("(fh_meta(f.metadata)->>'{k}')"), "meta"),
        k => return Err(bad(format!("Unknown field \"{k}\"."))),
    })
}

fn val_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}
fn val_list(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().filter_map(val_str).collect(),
        other => val_str(other).into_iter().collect(),
    }
}

fn cond_sql(q: &mut Sql, field: &str, op: &str, value: &Value) -> Result<String, ApiError> {
    let (e, kind) = field_expr(field)?;
    let fl = SYS_FIELDS.iter().find(|(k, _, _)| *k == field).map(|(_, l, _)| l.to_string()).unwrap_or_else(|| field.replace('_', " "));
    let need = |v: Option<String>| v.filter(|x| !x.is_empty()).ok_or_else(|| bad(format!("The rule on {fl} needs a value.")));
    let num_e = |e: &str| match kind {
        "num" => e.to_string(),
        _ => format!("(CASE WHEN {e} ~ '^-?[0-9]+(\\.[0-9]+)?$' THEN ({e})::numeric END)"),
    };
    let date_e = |e: &str| match kind {
        "ts" => e.to_string(),
        _ => format!("(CASE WHEN {e} ~ '^[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}' THEN substr({e},1,10)::date END)"),
    };
    Ok(match (op, kind) {
        ("contains", "tags") => { let p = q.s(format!("%{}%", like_esc(&need(val_str(value))?))); format!("{e} ILIKE {p}") }
        ("eq", "tags") => { let p = q.s(need(val_str(value))?); format!("(CASE WHEN {e} ~ '^\\s*\\[' THEN {e}::jsonb ELSE '[]'::jsonb END) ? {p}") }
        ("eq", "ts") | ("ne", "ts") => return Err(bad("Use before / after / in last N days for dates.")),
        ("eq", _) => { let p = q.s(need(val_str(value))?); format!("{e} = {p}") }
        ("ne", _) => { let p = q.s(need(val_str(value))?); format!("({e} IS DISTINCT FROM {p})") }
        ("in", _) => { let l = val_list(value); if l.is_empty() { return Err(bad(format!("The rule on {fl} needs at least one value."))); } let p = q.a(l); format!("{e} = ANY({p}::text[])") }
        ("nin", _) => { let l = val_list(value); let p = q.a(l); format!("NOT ({e} = ANY({p}::text[]))") }
        ("contains", _) => { let p = q.s(format!("%{}%", like_esc(&need(val_str(value))?))); format!("{e} ILIKE {p}") }
        ("starts", _) => { let p = q.s(format!("{}%", like_esc(&need(val_str(value))?))); format!("{e} ILIKE {p}") }
        ("gt" | "gte" | "lt" | "lte", "ts") | ("before" | "after", _) => {
            let v = need(val_str(value))?;
            chrono::NaiveDate::parse_from_str(v.get(..10).unwrap_or(&v), "%Y-%m-%d").map_err(|_| bad(format!("{v} is not a date (YYYY-MM-DD).")))?;
            let p = q.s(v);
            let sym = match op { "gt" | "after" => ">", "gte" => ">=", "lt" | "before" => "<", _ => "<=" };
            format!("{} {sym} {p}::date", date_e(&e))
        }
        ("gt" | "gte" | "lt" | "lte", _) => {
            let v = need(val_str(value))?;
            v.parse::<f64>().map_err(|_| bad(format!("{v} is not a number.")))?;
            let p = q.s(v);
            let sym = match op { "gt" => ">", "gte" => ">=", "lt" => "<", _ => "<=" };
            format!("{} {sym} {p}::numeric", num_e(&e))
        }
        ("last_days", _) => {
            let n: i64 = val_str(value).and_then(|v| v.parse().ok()).filter(|n: &i64| *n > 0 && *n < 36500)
                .ok_or_else(|| bad("\"In last N days\" needs a whole number of days."))?;
            let p = q.s(n.to_string());
            format!("{} >= (now() - ({p}::int * interval '1 day'))::date", date_e(&e))
        }
        ("empty", _) => format!("({e} IS NULL OR {e}::text = '' OR {e}::text = '[]')"),
        ("not_empty", _) => format!("NOT ({e} IS NULL OR {e}::text = '' OR {e}::text = '[]')"),
        (o, _) => return Err(bad(format!("Unknown operator \"{o}\"."))),
    })
}

fn like_esc(s: &str) -> String { s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_") }

fn rules_sql(q: &mut Sql, mode: &str, rules: &[Rule], depth: usize) -> Result<String, ApiError> {
    if depth > 1 { return Err(bad("Rule groups can be nested one level only.")); }
    let mut parts = Vec::new();
    for r in rules {
        parts.push(match r {
            Rule::Cond { field, op, value } => cond_sql(q, field, op, value)?,
            Rule::Group { mode, rules } => {
                if rules.is_empty() { continue; }
                format!("({})", rules_sql(q, mode, rules, depth + 1)?)
            }
        });
    }
    if parts.is_empty() { return Ok("TRUE".into()); }
    let join = if mode == "any" { " OR " } else { " AND " };
    Ok(parts.join(join))
}

/// Base WHERE for the files of a virtual basket that `user` may see.
async fn base_where(db: &PgPool, user: &User, q: &mut Sql, sources: &[String], all_sources: bool, mode: &str, rules: &[Rule], vb_id: Option<&str>) -> Result<(), ApiError> {
    let mut w = String::from(" WHERE f.deleted_at IS NULL");
    if !all_sources {
        if sources.is_empty() { return Err(bad("Pick at least one source basket.")); }
        let p = q.a(sources.to_vec());
        w.push_str(&format!(" AND f.system_id = ANY({p}::text[])"));
    }
    if let Some(scope) = crate::auth::effective_system_ids(db, user).await? {
        let p = q.a(scope.clone());
        w.push_str(&format!(" AND f.system_id = ANY({p}::text[])"));
        let v = acl::visibility(db, user, &scope).await?;
        let bp = q.a(v.baskets);
        let fp = q.a(v.folders);
        let fip = q.a(v.files);
        let up = q.a(vec![v.uid]);
        w.push_str(&format!(" AND ((f.folder_id IS NULL AND f.system_id = ANY({bp}::text[])) OR f.folder_id = ANY({fp}::text[]) OR f.id::text = ANY({fip}::text[]) OR f.created_by = ({up}::text[])[1])"));
    }
    if let Some(id) = vb_id {
        let p = q.s(id.to_string());
        w.push_str(&format!(" AND f.id NOT IN (SELECT file_id FROM vbasket_excludes WHERE vbasket_id = {p})"));
    }
    let r = rules_sql(q, mode, rules, 0)?;
    w.push_str(&format!(" AND ({r})"));
    q.text.push_str(&w);
    Ok(())
}

fn bind_all<'q, O>(mut x: sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>, binds: &'q [Bind])
    -> sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments> {
    for b in binds { x = match b { Bind::S(s) => x.bind(s), Bind::A(a) => x.bind(a) }; }
    x
}
fn bind_scalar<'q, O>(mut x: sqlx::query::QueryScalar<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>, binds: &'q [Bind])
    -> sqlx::query::QueryScalar<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments> {
    for b in binds { x = match b { Bind::S(s) => x.bind(s), Bind::A(a) => x.bind(a) }; }
    x
}

// -----------------------------------------------------------------------------
// Access to a virtual basket
// -----------------------------------------------------------------------------

async fn load(db: &PgPool, user: &User, id: &str) -> Result<(VRow, bool), ApiError> {
    let row: VRow = sqlx::query_as(
        "SELECT id, name, description, match_mode, rules, group_by, all_sources, drop_target_system_id, drop_target_folder_id, columns, personal, shared_with, created_by, created_at, updated_at FROM vbaskets WHERE id = $1 AND deleted_at IS NULL",
    ).bind(id).fetch_optional(db).await?.ok_or(ApiError::NotFound)?;
    let can_edit = user.role == "admin" || row.created_by == user.id;
    let can_open = can_edit || (!row.personal && (row.shared_with.is_empty() || {
        let p = acl::principals(db, user).await?;
        row.shared_with.iter().any(|s| p.contains(s))
    }));
    if !can_open { return Err(ApiError::NotFound); }
    Ok((row, can_edit))
}

async fn sources_of(db: &PgPool, id: &str) -> Result<Vec<String>, ApiError> {
    Ok(sqlx::query_scalar("SELECT system_id FROM vbasket_sources WHERE vbasket_id = $1 ORDER BY system_id").bind(id).fetch_all(db).await?)
}

fn prefill_of(mode: &str, rules: &[Rule]) -> Value {
    let mut out = serde_json::Map::new();
    let mut fields = serde_json::Map::new();
    if mode != "all" { return json!({}); }
    for r in rules {
        if let Rule::Cond { field, op, value } = r {
            let one = match op.as_str() {
                "eq" => val_str(value),
                "in" => { let l = val_list(value); if l.len() == 1 { l.into_iter().next() } else { None } }
                _ => None,
            };
            let Some(v) = one else { continue };
            match field.as_str() {
                "sys.category" => { out.insert("category_code".into(), json!(v)); }
                "sys.status" => { out.insert("status".into(), json!(v)); }
                "sys.org" => { out.insert("org_id".into(), json!(v)); }
                k if valid_key(k) => { fields.insert(k.into(), json!(v)); }
                _ => {}
            }
        }
    }
    out.insert("fields".into(), Value::Object(fields));
    Value::Object(out)
}

async fn to_view(db: &PgPool, row: VRow, can_edit: bool) -> Result<VBasket, ApiError> {
    let sources = sources_of(db, &row.id).await?;
    let source_names: Vec<String> = if row.all_sources { vec!["All baskets".into()] } else {
        sqlx::query_scalar("SELECT name FROM systems WHERE id = ANY($1) ORDER BY name").bind(&sources).fetch_all(db).await?
    };
    let drop_target_label = match (&row.drop_target_system_id, &row.drop_target_folder_id) {
        (Some(s), Some(f)) => {
            let a: Option<String> = sqlx::query_scalar("SELECT name FROM systems WHERE id = $1").bind(s).fetch_optional(db).await?;
            let b: Option<String> = sqlx::query_scalar("SELECT name FROM folders WHERE id = $1").bind(f).fetch_optional(db).await?;
            Some(format!("{} › {}", a.unwrap_or_default(), b.unwrap_or_default()))
        }
        (Some(s), None) => sqlx::query_scalar("SELECT name FROM systems WHERE id = $1").bind(s).fetch_optional(db).await?,
        _ => None,
    };
    let rules: Vec<Rule> = serde_json::from_value(row.rules.clone()).unwrap_or_default();
    let prefill = prefill_of(&row.match_mode, &rules);
    let mut group_by_labels = Vec::new();
    for g in &row.group_by { group_by_labels.push(field_label(db, g, &sources).await?); }
    Ok(VBasket { row, sources, source_names, can_edit, drop_target_label, group_by_labels, prefill })
}

// -----------------------------------------------------------------------------
// CRUD
// -----------------------------------------------------------------------------

pub async fn list(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Vec<VBasket>>> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM vbaskets WHERE deleted_at IS NULL ORDER BY personal, name").fetch_all(&s.db).await?;
    let mut out = Vec::new();
    for id in ids {
        if let Ok((row, can_edit)) = load(&s.db, &user.0, &id).await { out.push(to_view(&s.db, row, can_edit).await?); }
    }
    Ok(Json(out))
}

pub async fn get_one(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>) -> ApiResult<Json<VBasket>> {
    let (row, can_edit) = load(&s.db, &user.0, &id).await?;
    Ok(Json(to_view(&s.db, row, can_edit).await?))
}

async fn validate_in(db: &PgPool, user: &User, b: &VBasketIn) -> Result<(), ApiError> {
    if b.name.trim().is_empty() || b.name.len() > 120 { return Err(bad("Name must be 1–120 characters.")); }
    if b.match_mode != "all" && b.match_mode != "any" { return Err(bad("Match must be all or any.")); }
    if !b.all_sources && b.sources.is_empty() { return Err(bad("Pick at least one source basket.")); }
    for sid in &b.sources {
        let lvl = acl::folder_level(db, user, sid, None).await?;
        if lvl < acl::VIEW {
            // Still allowed if the user can see something inside (folder/file grants) — check existence only.
            let ok: Option<(String,)> = sqlx::query_as("SELECT id FROM systems WHERE id = $1 AND deleted_at IS NULL").bind(sid).fetch_optional(db).await?;
            if ok.is_none() { return Err(bad(format!("Basket {sid} does not exist."))); }
        }
    }
    if b.group_by.len() > 4 { return Err(bad("Group by up to 4 levels.")); }
    for g in &b.group_by { field_expr(g)?; }
    // Compile once so an invalid rule is refused at save time, not when opened.
    let mut q = Sql::new();
    rules_sql(&mut q, &b.match_mode, &b.rules, 0)?;
    if let Some(t) = &b.drop_target_system_id {
        acl::require_folder(db, user, t, b.drop_target_folder_id.as_deref(), acl::EDIT).await
            .map_err(|_| bad("You need Edit on the drop target."))?;
        if !b.all_sources && !b.sources.contains(t) { return Err(bad("The drop target must be one of the sources.")); }
    }
    if !b.personal && user.role == "viewer" { return Err(bad("Viewers can create personal virtual baskets only.")); }
    Ok(())
}

pub async fn create(State(s): State<Arc<AppState>>, user: AuthUser, Json(b): Json<VBasketIn>) -> ApiResult<Json<VBasket>> {
    validate_in(&s.db, &user.0, &b).await?;
    let id = crate::new_id();
    save(&s.db, &id, &b, Some(&user.0.id)).await?;
    get_one(State(s), user, Path(id)).await
}

pub async fn update(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>, Json(b): Json<VBasketIn>) -> ApiResult<Json<VBasket>> {
    let (_, can_edit) = load(&s.db, &user.0, &id).await?;
    if !can_edit { return Err(ApiError::Forbidden); }
    validate_in(&s.db, &user.0, &b).await?;
    save(&s.db, &id, &b, None).await?;
    get_one(State(s), user, Path(id)).await
}

async fn save(db: &PgPool, id: &str, b: &VBasketIn, creator: Option<&str>) -> Result<(), ApiError> {
    let mut tx = db.begin().await?;
    let rules = serde_json::to_value(&b.rules).unwrap_or(json!([]));
    if let Some(c) = creator {
        sqlx::query(
            r#"INSERT INTO vbaskets (id, name, description, match_mode, rules, group_by, all_sources, drop_target_system_id,
                                     drop_target_folder_id, columns, personal, shared_with, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)"#,
        ).bind(id).bind(b.name.trim()).bind(&b.description).bind(&b.match_mode).bind(&rules).bind(&b.group_by)
        .bind(b.all_sources).bind(&b.drop_target_system_id).bind(&b.drop_target_folder_id).bind(&b.columns)
        .bind(b.personal).bind(&b.shared_with).bind(c).execute(&mut *tx).await?;
    } else {
        sqlx::query(
            r#"UPDATE vbaskets SET name=$2, description=$3, match_mode=$4, rules=$5, group_by=$6, all_sources=$7,
                 drop_target_system_id=$8, drop_target_folder_id=$9, columns=$10, personal=$11, shared_with=$12, updated_at=now()
               WHERE id=$1"#,
        ).bind(id).bind(b.name.trim()).bind(&b.description).bind(&b.match_mode).bind(&rules).bind(&b.group_by)
        .bind(b.all_sources).bind(&b.drop_target_system_id).bind(&b.drop_target_folder_id).bind(&b.columns)
        .bind(b.personal).bind(&b.shared_with).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM vbasket_sources WHERE vbasket_id = $1").bind(id).execute(&mut *tx).await?;
    }
    for sid in &b.sources {
        sqlx::query("INSERT INTO vbasket_sources (vbasket_id, system_id) VALUES ($1,$2) ON CONFLICT DO NOTHING")
            .bind(id).bind(sid).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn remove(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let (row, can_edit) = load(&s.db, &user.0, &id).await?;
    if !can_edit { return Err(ApiError::Forbidden); }
    // Only the saved view goes; the files it shows stay where they are.
    sqlx::query("UPDATE vbaskets SET deleted_at = now() WHERE id = $1").bind(&id).execute(&s.db).await?;
    crate::basket::log_activity(&s.db, &user.0, "deleted virtual basket", &row.name, "vbasket", None, "").await;
    Ok(StatusCode::NO_CONTENT)
}

// -----------------------------------------------------------------------------
// Preview / tree / fields / excludes
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct ItemRow {
    id: Id,
    name: String,
    system_id: String,
    basket_name: String,
    size_bytes: i64,
    status: String,
    category_code: Option<String>,
    modified_at: chrono::DateTime<chrono::Utc>,
    folder_id: Option<String>,
    metadata: String,
    created_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Preview { count: i64, bytes: i64, items: Vec<ItemRow> }

pub async fn preview(State(s): State<Arc<AppState>>, user: AuthUser, Json(b): Json<VBasketIn>) -> ApiResult<Json<Preview>> {
    let mut q = Sql::new();
    base_where(&s.db, &user.0, &mut q, &b.sources, b.all_sources, &b.match_mode, &b.rules, None).await?;
    let w = q.text.clone();
    let (count, bytes): (i64, i64) = bind_all(sqlx::query_as(&format!("SELECT COUNT(*), COALESCE(SUM(f.size_bytes),0)::bigint FROM files f{w}")), &q.binds).fetch_one(&s.db).await?;
    let items = bind_all(sqlx::query_as::<_, ItemRow>(&format!(
        "SELECT f.id, f.name, f.system_id, s.name AS basket_name, f.size_bytes, f.status, f.category_code, f.modified_at, f.folder_id, f.metadata, f.created_by FROM files f JOIN systems s ON s.id = f.system_id{w} ORDER BY f.modified_at DESC LIMIT 20")), &q.binds)
        .fetch_all(&s.db).await?;
    Ok(Json(Preview { count, bytes, items }))
}

#[derive(Debug, Deserialize)]
pub struct TreeQuery { pub path: Option<String>, pub q: Option<String>, pub limit: Option<i64>, pub offset: Option<i64>, pub sort: Option<String>, pub dir: Option<String> }

#[derive(Debug, Serialize)]
pub struct Group { value: Option<String>, label: String, count: i64, bytes: i64, modified_at: Option<chrono::DateTime<chrono::Utc>> }

#[derive(Debug, Serialize)]
pub struct Tree {
    level: usize,
    level_field: Option<String>,
    level_label: Option<String>,
    path: Vec<Crumb>,
    groups: Vec<Group>,
    items: Vec<ItemRow>,
    total: i64,
    bytes: i64,
}
#[derive(Debug, Serialize)]
pub struct Crumb { value: Option<String>, label: String }

const NONE_TOKEN: &str = "__none__";

/// Virtual-basket pages are grouped aggregates over many files; a short cache
/// per user+view absorbs bursts (load test: Postgres CPU was the bottleneck).
fn tree_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, Value)>> {
    static C: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, Value)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}
const TREE_TTL: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn tree(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>, Query(tq): Query<TreeQuery>) -> ApiResult<Json<Value>> {
    let key = format!("{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}", user.0.id, id, tq.path, tq.q, tq.sort, tq.dir, tq.limit, tq.offset);
    if let Some((t, v)) = tree_cache().lock().unwrap().get(&key).cloned() {
        if t.elapsed() < TREE_TTL { return Ok(Json(v)); }
    }
    let Json(t) = tree_uncached(State(s), user, Path(id), Query(tq)).await?;
    let v = serde_json::to_value(&t).unwrap_or(Value::Null);
    let mut c = tree_cache().lock().unwrap();
    if c.len() > 5000 { c.retain(|_, (t, _)| t.elapsed() < TREE_TTL); }
    c.insert(key, (std::time::Instant::now(), v.clone()));
    Ok(Json(v))
}

async fn tree_uncached(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>, Query(tq): Query<TreeQuery>) -> ApiResult<Json<Tree>> {
    let (row, _) = load(&s.db, &user.0, &id).await?;
    let sources = sources_of(&s.db, &id).await?;
    let rules: Vec<Rule> = serde_json::from_value(row.rules.clone()).map_err(|_| bad("This virtual basket's rules are damaged. Edit and save them again."))?;
    let path: Vec<String> = tq.path.as_deref().unwrap_or("").split('/').filter(|p| !p.is_empty())
        .map(|p| urlencoding::decode(p).map(|c| c.into_owned()).unwrap_or_default()).collect();
    if path.len() > row.group_by.len() { return Err(ApiError::NotFound); }

    let mut q = Sql::new();
    base_where(&s.db, &user.0, &mut q, &sources, row.all_sources, &row.match_mode, &rules, Some(&id)).await?;
    let mut crumbs = Vec::new();
    for (i, v) in path.iter().enumerate() {
        let (e, _) = field_expr(&row.group_by[i])?;
        if v == NONE_TOKEN {
            q.text.push_str(&format!(" AND ({e} IS NULL OR {e}::text = '')"));
            crumbs.push(Crumb { value: None, label: format!("No {}", field_label(&s.db, &row.group_by[i], &sources).await?.to_lowercase()) });
        } else {
            let p = q.s(v.clone());
            q.text.push_str(&format!(" AND {e}::text = {p}"));
            crumbs.push(Crumb { value: Some(v.clone()), label: value_label(&s.db, &row.group_by[i], v).await? });
        }
    }
    if let Some(term) = tq.q.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let p = q.s(format!("%{}%", like_esc(term)));
        q.text.push_str(&format!(" AND (f.name ILIKE {p} OR f.metadata ILIKE {p} OR f.tags ILIKE {p})"));
    }
    let w = q.text.clone();
    let (total, bytes): (i64, i64) = bind_all(sqlx::query_as(&format!("SELECT COUNT(*), COALESCE(SUM(f.size_bytes),0)::bigint FROM files f{w}")), &q.binds).fetch_one(&s.db).await?;

    let level = path.len();
    if level < row.group_by.len() {
        let field = row.group_by[level].clone();
        let (e, _) = field_expr(&field)?;
        let rows: Vec<(Option<String>, i64, i64, Option<chrono::DateTime<chrono::Utc>>)> = bind_all(sqlx::query_as(&format!(
            "SELECT NULLIF({e}::text, '') AS v, COUNT(*), COALESCE(SUM(f.size_bytes),0)::bigint, MAX(f.modified_at) FROM files f{w} GROUP BY 1")), &q.binds)
            .fetch_all(&s.db).await?;
        let flabel = field_label(&s.db, &field, &sources).await?;
        let mut groups = Vec::new();
        for (v, c, b, m) in rows {
            let label = match &v { Some(x) => value_label(&s.db, &field, x).await?, None => format!("No {}", flabel.to_lowercase()) };
            groups.push(Group { value: v, label, count: c, bytes: b, modified_at: m });
        }
        groups.sort_by(|a, b| (a.value.is_none(), a.label.to_lowercase()).cmp(&(b.value.is_none(), b.label.to_lowercase())));
        return Ok(Json(Tree { level, level_field: Some(field), level_label: Some(flabel), path: crumbs, groups, items: vec![], total, bytes }));
    }
    let sort = match tq.sort.as_deref() { Some("name") => "f.name", Some("size") => "f.size_bytes", Some("status") => "f.status", _ => "f.modified_at" };
    let dir = if tq.dir.as_deref() == Some("asc") { "ASC" } else { "DESC" };
    let limit = tq.limit.unwrap_or(200).clamp(1, 1000);
    let offset = tq.offset.unwrap_or(0).max(0);
    let lp = q.s(limit.to_string());
    let op = q.s(offset.to_string());
    let items = bind_all(sqlx::query_as::<_, ItemRow>(&format!(
        "SELECT f.id, f.name, f.system_id, s.name AS basket_name, f.size_bytes, f.status, f.category_code, f.modified_at, f.folder_id, f.metadata, f.created_by FROM files f JOIN systems s ON s.id = f.system_id{w} ORDER BY {sort} {dir}, f.id LIMIT {lp}::bigint OFFSET {op}::bigint")), &q.binds)
        .fetch_all(&s.db).await?;
    Ok(Json(Tree { level, level_field: None, level_label: None, path: crumbs, groups: vec![], items, total, bytes }))
}

async fn field_label(db: &PgPool, field: &str, sources: &[String]) -> Result<String, ApiError> {
    if let Some((_, l, _)) = SYS_FIELDS.iter().find(|(k, _, _)| *k == field) { return Ok(l.to_string()); }
    Ok(sqlx::query_scalar::<_, String>("SELECT label FROM basket_fields WHERE key = $1 AND (system_id = ANY($2) OR $3) LIMIT 1")
        .bind(field).bind(sources).bind(sources.is_empty()).fetch_optional(db).await?.unwrap_or_else(|| field.to_string()))
}

async fn value_label(db: &PgPool, field: &str, v: &str) -> Result<String, ApiError> {
    let l: Option<String> = match field {
        "sys.basket" => sqlx::query_scalar("SELECT name FROM systems WHERE id = $1").bind(v).fetch_optional(db).await?,
        "sys.org" => sqlx::query_scalar("SELECT name FROM orgs WHERE id = $1").bind(v).fetch_optional(db).await?,
        "sys.category" => sqlx::query_scalar("SELECT name FROM basket_categories WHERE code = $1 LIMIT 1").bind(v).fetch_optional(db).await?,
        "sys.status" => sqlx::query_scalar("SELECT label FROM basket_statuses WHERE code = $1 LIMIT 1").bind(v).fetch_optional(db).await?,
        "sys.created_by" => sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1").bind(v).fetch_optional(db).await?,
        _ => None,
    };
    Ok(l.unwrap_or_else(|| v.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct FieldsQuery { pub sources: Option<String>, pub all: Option<bool> }

#[derive(Debug, Serialize)]
pub struct FieldOption { key: String, label: String, kind: String, choices: Vec<Value>, system: bool }

/// Fields usable in rules and Group by for the given sources: system fields plus
/// basket fields every source has.
pub async fn fields(State(s): State<Arc<AppState>>, user: AuthUser, Query(fq): Query<FieldsQuery>) -> ApiResult<Json<Vec<FieldOption>>> {
    let mut sources: Vec<String> = fq.sources.unwrap_or_default().split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect();
    if fq.all.unwrap_or(false) || sources.is_empty() {
        sources = match crate::auth::effective_system_ids(&s.db, &user.0).await? {
            Some(v) => v,
            None => sqlx::query_scalar("SELECT id FROM systems WHERE deleted_at IS NULL").fetch_all(&s.db).await?,
        };
    }
    let mut out = Vec::new();
    for (k, l, kind) in SYS_FIELDS {
        let choices: Vec<Value> = match *k {
            "sys.format" => FORMATS.iter().map(|f| json!({"value": f, "label": f})).collect(),
            "sys.basket" => sqlx::query_as::<_, (String, String)>("SELECT id, name FROM systems WHERE id = ANY($1) ORDER BY name").bind(&sources).fetch_all(&s.db).await?
                .into_iter().map(|(v, l)| json!({"value": v, "label": l})).collect(),
            "sys.category" => sqlx::query_as::<_, (String, String)>("SELECT DISTINCT code, name FROM basket_categories WHERE system_id = ANY($1) ORDER BY name").bind(&sources).fetch_all(&s.db).await?
                .into_iter().map(|(v, l)| json!({"value": v, "label": l})).collect(),
            "sys.status" => sqlx::query_as::<_, (String, String)>("SELECT DISTINCT code, label FROM basket_statuses WHERE system_id = ANY($1) ORDER BY label").bind(&sources).fetch_all(&s.db).await?
                .into_iter().map(|(v, l)| json!({"value": v, "label": l})).collect(),
            _ => vec![],
        };
        out.push(FieldOption { key: k.to_string(), label: l.to_string(), kind: kind.to_string(), choices, system: true });
    }
    let n = sources.len() as i64;
    let rows: Vec<(String, String, String, Value)> = sqlx::query_as(
        r#"SELECT key, MIN(label), MIN(field_type), (array_agg(choices))[1] FROM basket_fields
            WHERE system_id = ANY($1) GROUP BY key HAVING COUNT(DISTINCT system_id) = $2 ORDER BY MIN(label)"#,
    ).bind(&sources).bind(n).fetch_all(&s.db).await?;
    for (key, label, kind, choices) in rows {
        let choices = choices.as_array().cloned().unwrap_or_default().into_iter()
            .map(|c| if c.is_object() { c } else { json!({"value": c, "label": c}) }).collect();
        out.push(FieldOption { key, label, kind, choices, system: false });
    }
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
pub struct ExcludeReq { pub file_id: Id }

pub async fn exclude(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>, Json(b): Json<ExcludeReq>) -> ApiResult<StatusCode> {
    let (_, can_edit) = load(&s.db, &user.0, &id).await?;
    if !can_edit { return Err(ApiError::Forbidden); }
    sqlx::query("INSERT INTO vbasket_excludes (vbasket_id, file_id, excluded_by) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(&id).bind(b.file_id).bind(&user.0.id).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unexclude(State(s): State<Arc<AppState>>, user: AuthUser, Path((id, fid)): Path<(String, Id)>) -> ApiResult<StatusCode> {
    let (_, can_edit) = load(&s.db, &user.0, &id).await?;
    if !can_edit { return Err(ApiError::Forbidden); }
    sqlx::query("DELETE FROM vbasket_excludes WHERE vbasket_id = $1 AND file_id = $2").bind(&id).bind(fid).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Which virtual baskets a file shows up in (for the "Appears in" tab).
pub async fn appears_in(State(s): State<Arc<AppState>>, user: AuthUser, Path(fid): Path<Id>) -> ApiResult<Json<Vec<Value>>> {
    acl::require_file(&s.db, &user.0, fid, acl::VIEW).await?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM vbaskets WHERE deleted_at IS NULL").fetch_all(&s.db).await?;
    let mut out = Vec::new();
    for id in ids {
        let Ok((row, _)) = load(&s.db, &user.0, &id).await else { continue };
        let Ok(rules) = serde_json::from_value::<Vec<Rule>>(row.rules.clone()) else { continue };
        let sources = sources_of(&s.db, &id).await?;
        let mut q = Sql::new();
        if base_where(&s.db, &user.0, &mut q, &sources, row.all_sources, &row.match_mode, &rules, Some(&id)).await.is_err() { continue; }
        let p = q.s(fid.to_string());
        let w = format!("{} AND f.id = {p}", q.text);
        let hit: i64 = bind_scalar(sqlx::query_scalar(&format!("SELECT COUNT(*) FROM files f{w}")), &q.binds).fetch_one(&s.db).await?;
        if hit > 0 {
            out.push(json!({ "id": row.id, "name": row.name, "match_mode": row.match_mode, "rules": row.rules, "group_by": row.group_by }));
        }
    }
    Ok(Json(out))
}

// -----------------------------------------------------------------------------
// Download list
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DlItem {
    File { id: Id },
    Folder { id: String },
    Vgroup { vbasket_id: String, #[serde(default)] path: Vec<String> },
}

pub async fn get_downloads(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Value>> {
    let items: Option<Value> = sqlx::query_scalar("SELECT items FROM download_lists WHERE user_id = $1").bind(&user.0.id).fetch_optional(&s.db).await?;
    let items: Vec<DlItem> = serde_json::from_value(items.unwrap_or(json!([]))).unwrap_or_default();
    let mut out = Vec::new();
    for it in items {
        let (label, files) = match &it {
            DlItem::File { id } => match sqlx::query_scalar::<_, String>("SELECT name FROM files WHERE id = $1 AND deleted_at IS NULL").bind(id).fetch_optional(&s.db).await? {
                Some(n) => (n, 1),
                None => continue,
            },
            DlItem::Folder { id } => {
                let n: Option<String> = sqlx::query_scalar("SELECT name FROM folders WHERE id = $1 AND deleted_at IS NULL").bind(id).fetch_optional(&s.db).await?;
                let Some(n) = n else { continue };
                let c = resolve(&s, &user.0, &it).await.map(|v| v.len()).unwrap_or(0);
                (n, c)
            }
            DlItem::Vgroup { vbasket_id, path } => {
                let n: Option<String> = sqlx::query_scalar("SELECT name FROM vbaskets WHERE id = $1 AND deleted_at IS NULL").bind(vbasket_id).fetch_optional(&s.db).await?;
                let Some(n) = n else { continue };
                let c = resolve(&s, &user.0, &it).await.map(|v| v.len()).unwrap_or(0);
                let gb: Vec<String> = sqlx::query_scalar::<_, Vec<String>>("SELECT group_by FROM vbaskets WHERE id = $1").bind(vbasket_id).fetch_one(&s.db).await?;
                let srcs = sources_of(&s.db, vbasket_id).await?;
                let mut labels = Vec::new();
                for (i, v) in path.iter().enumerate() {
                    let f = gb.get(i).cloned().unwrap_or_default();
                    labels.push(if v == NONE_TOKEN { format!("No {}", field_label(&s.db, &f, &srcs).await?.to_lowercase()) } else { value_label(&s.db, &f, v).await? });
                }
                (if path.is_empty() { n } else { format!("{n} › {}", labels.join(" › ")) }, c)
            }
        };
        out.push(json!({ "item": it, "label": label, "files": files }));
    }
    Ok(Json(json!(out)))
}

pub async fn put_downloads(State(s): State<Arc<AppState>>, user: AuthUser, Json(items): Json<Vec<DlItem>>) -> ApiResult<Json<Value>> {
    if items.len() > 500 { return Err(bad("A download list holds up to 500 items.")); }
    sqlx::query(
        "INSERT INTO download_lists (user_id, items, updated_at) VALUES ($1,$2,now()) ON CONFLICT (user_id) DO UPDATE SET items = EXCLUDED.items, updated_at = now()",
    ).bind(&user.0.id).bind(serde_json::to_value(&items).unwrap()).execute(&s.db).await?;
    get_downloads(State(s), user).await
}

/// Files of one list item as (zip folder path, File). Viewer-visible only.
async fn resolve(s: &Arc<AppState>, user: &User, it: &DlItem) -> Result<Vec<(String, File)>, ApiError> {
    match it {
        DlItem::File { id } => {
            if acl::require_file(&s.db, user, *id, acl::VIEW).await.is_err() { return Ok(vec![]); }
            let f: Option<File> = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL")).bind(id).fetch_optional(&s.db).await?;
            Ok(f.into_iter().map(|f| (String::new(), f)).collect())
        }
        DlItem::Folder { id } => {
            let rows: Vec<(String, String)> = sqlx::query_as(
                r#"WITH RECURSIVE t AS (SELECT id, name::text AS p FROM folders WHERE id = $1 AND deleted_at IS NULL
                     UNION ALL SELECT f.id, t.p || '/' || f.name FROM folders f JOIN t ON f.parent_id = t.id WHERE f.deleted_at IS NULL)
                   SELECT id, p FROM t"#,
            ).bind(id).fetch_all(&s.db).await?;
            let mut out = Vec::new();
            for (fid, p) in rows {
                let files: Vec<File> = sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE folder_id = $1 AND deleted_at IS NULL")).bind(&fid).fetch_all(&s.db).await?;
                for f in files {
                    if acl::file_level(&s.db, user, f.id).await.unwrap_or(0) >= acl::VIEW { out.push((p.clone(), f)); }
                }
            }
            Ok(out)
        }
        DlItem::Vgroup { vbasket_id, path } => {
            let (row, _) = load(&s.db, user, vbasket_id).await?;
            let sources = sources_of(&s.db, vbasket_id).await?;
            let rules: Vec<Rule> = serde_json::from_value(row.rules.clone()).unwrap_or_default();
            let mut q = Sql::new();
            base_where(&s.db, user, &mut q, &sources, row.all_sources, &row.match_mode, &rules, Some(vbasket_id)).await?;
            for (i, v) in path.iter().enumerate() {
                let Some(g) = row.group_by.get(i) else { break };
                let (e, _) = field_expr(g)?;
                if v == NONE_TOKEN { q.text.push_str(&format!(" AND ({e} IS NULL OR {e}::text = '')")); }
                else { let p = q.s(v.clone()); q.text.push_str(&format!(" AND {e}::text = {p}")); }
            }
            // Remaining group levels become the zip folders below the item.
            let rest: Vec<String> = row.group_by.iter().skip(path.len()).cloned().collect();
            let mut sel = String::new();
            for g in &rest { let (e, _) = field_expr(g)?; sel.push_str(&format!(", COALESCE(NULLIF({e}::text,''), '_none') AS lv")); break; }
            let cols = FILE_COLS.split(", ").map(|c| format!("f.{c}")).collect::<Vec<_>>().join(", ");
            let files: Vec<File> = bind_all(sqlx::query_as(&format!("SELECT {cols} FROM files f{} LIMIT 5000", q.text)), &q.binds).fetch_all(&s.db).await?;
            let mut out = Vec::new();
            for f in files {
                let mut parts = vec![row.name.clone()];
                for (i, v) in path.iter().enumerate() {
                    let g = row.group_by.get(i).cloned().unwrap_or_default();
                    parts.push(if v == NONE_TOKEN { format!("No {}", field_label(&s.db, &g, &sources).await?.to_lowercase()) } else { value_label(&s.db, &g, v).await? });
                }
                let meta: Value = serde_json::from_str(&f.metadata).unwrap_or(json!({}));
                for g in &rest {
                    let v = match g.as_str() {
                        "sys.basket" => Some(f.system_id.clone()),
                        "sys.status" => Some(f.status.clone()),
                        "sys.category" => f.category_code.clone(),
                        "sys.org" => f.org_id.clone(),
                        "sys.year" => Some(f.created_at.format("%Y").to_string()),
                        "sys.month" => Some(f.created_at.format("%Y-%m").to_string()),
                        k if valid_key(k) => meta.get(k).and_then(val_str),
                        _ => None,
                    };
                    parts.push(match v { Some(x) => value_label(&s.db, g, &x).await?, None => format!("No {}", field_label(&s.db, g, &sources).await?.to_lowercase()) });
                }
                let _ = &sel;
                out.push((parts.join("/"), f));
            }
            Ok(out)
        }
    }
}

/// The whole folder (and every folder below it) as one zip.
pub async fn folder_zip(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<String>) -> ApiResult<axum::response::Response> {
    let (name, sys): (String, String) = sqlx::query_as("SELECT name, system_id FROM folders WHERE id = $1 AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    crate::acl::require_folder(&s.db, &user.0, &sys, Some(&id), crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    let r = zip_items(&s, &user, vec![DlItem::Folder { id }], Some(name)).await;
    match r {
        Ok(v) => Ok(v.into_response()),
        Err(ApiError::BadRequest(m)) if m.starts_with("None of the files") => Err(ApiError::BadRequest("โฟลเดอร์นี้ไม่มีไฟล์ให้ดาวน์โหลด".into())),
        Err(e) => Err(e),
    }
}

pub async fn download_zip(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<impl IntoResponse> {
    let items: Option<Value> = sqlx::query_scalar("SELECT items FROM download_lists WHERE user_id = $1").bind(&user.0.id).fetch_optional(&s.db).await?;
    let items: Vec<DlItem> = serde_json::from_value(items.unwrap_or(json!([]))).unwrap_or_default();
    if items.is_empty() { return Err(bad("Your download list is empty.")); }
    zip_items(&s, &user, items, None).await
}

async fn zip_items(s: &Arc<AppState>, user: &AuthUser, items: Vec<DlItem>, zip_name: Option<String>) -> ApiResult<(HeaderMap, axum::body::Body)> {
    let mut all = Vec::new();
    let mut seen = HashSet::new();
    let mut held_back = 0usize;
    for it in &items {
        for (p, f) in resolve(s, &user.0, it).await? {
            if !seen.insert(f.id) { continue; }
            // Files that must go out stamped (policy, or any Vault) never leave
            // in a zip as the original — they are downloaded one by one.
            let pol = crate::basket::effective_policy(&s.db, &f.system_id, f.folder_id.as_deref()).await?;
            let stamp_on = pol.stamp_downloads == Some(true) || crate::basket::is_vault(&s.db, &f.system_id).await?;
            if stamp_on && matches!(f.file_type.as_str(), "pdf" | "docx" | "xlsx" | "pptx") { held_back += 1; continue; }
            all.push((p, f));
        }
    }
    if all.is_empty() {
        return Err(bad(if held_back > 0 { "These files are stamped on download — download them one by one." } else { "None of the files in your list are available to you." }));
    }
    let total: i64 = all.iter().map(|(_, f)| f.size_bytes).sum();
    let cap: i64 = std::env::var("ZIP_MAX_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(2048) * 1024 * 1024;
    if total > cap { return Err(ApiError::PayloadTooLarge(format!("The list is {} — more than the {} a zip may hold. Split it.", crate::basket::human_bytes(total), crate::basket::human_bytes(cap)))); }

    // Build into a temp file so memory doesn't scale with the archive.
    let tmp = tempfile_path();
    let file = std::fs::File::create(&tmp).map_err(|e| ApiError::Other(e.into()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for (dir, f) in &all {
        let Some(mut content) = s.storage.open_stream(&f.object_key, f.encrypted).await? else { continue };
        // Entry paths never climb out of the folder they are extracted into.
        let clean = |x: &str| { let c = x.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"); if c.trim_matches('.').is_empty() { "_".to_string() } else { c } };
        let mut name = if dir.is_empty() { clean(&f.name) } else { format!("{}/{}", dir.split('/').filter(|p| !p.is_empty()).map(clean).collect::<Vec<_>>().join("/"), clean(&f.name)) };
        let n = names.entry(name.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            let (stem, ext) = match name.rfind('.') { Some(i) => (name[..i].to_string(), name[i..].to_string()), None => (name.clone(), String::new()) };
            name = format!("{stem} ({}){ext}", *n);
        }
        use chrono::{Datelike, Timelike};
        let m = f.modified_at;
        let o = match zip::DateTime::from_date_and_time(m.year() as u16, m.month() as u8, m.day() as u8, m.hour() as u8, m.minute() as u8, m.second() as u8) {
            Ok(dt) => opts.last_modified_time(dt),
            Err(_) => opts,
        };
        crate::basket::log_vault_access(&s.db, &user.0, &f.system_id, f.id, &f.name, "downloaded in a zip").await;
        zip.start_file(name, o).map_err(|e| ApiError::Other(e.into()))?;
        use futures::StreamExt;
        while let Some(chunk) = content.next().await {
            zip.write_all(&chunk.map_err(|e| ApiError::Other(e.into()))?).map_err(|e| ApiError::Other(e.into()))?;
        }
    }
    zip.finish().map_err(|e| ApiError::Other(e.into()))?;
    // Stream the archive out; the file is unlinked at once (the open handle keeps it readable).
    let archive = tokio::fs::File::open(&tmp).await.map_err(|e| ApiError::Other(e.into()))?;
    let _ = tokio::fs::remove_file(&tmp).await;
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(archive));
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, "application/zip".parse().unwrap());
    let fname = match &zip_name {
        Some(n) => format!("{}.zip", n.replace(['/', '\\', '"', ':'], "_")),
        None => format!("filehub-{}.zip", chrono::Utc::now().format("%Y%m%d-%H%M")),
    };
    let enc: String = fname.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    h.insert(header::CONTENT_DISPOSITION, format!("attachment; filename=\"folder.zip\"; filename*=UTF-8''{enc}").parse().unwrap());
    h.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    crate::basket::log_activity(&s.db, &user.0, &format!("downloaded zip of {} files", all.len()), &fname, "zip", None, "").await;
    Ok((h, body))
}

fn tempfile_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("filehub-zip-{}.zip", crate::Id::new()))
}

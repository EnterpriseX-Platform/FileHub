//! Object lock (WORM): a file that must not be deleted until a date, or while
//! on legal hold — for records that the law says must be kept.
//!
//! - COMPLIANCE: nobody can delete it before `retain_until`, or shorten that
//!   date, or switch it to GOVERNANCE — not even an admin.
//! - GOVERNANCE: the same, except an admin may delete it or change the lock.
//! - legal hold: can't be deleted while set, whatever the date. People with
//!   Manage on the file set and lift it.
//!
//! A basket or folder policy can lock every new file for N days. New versions
//! may still be uploaded: earlier versions are kept, and stay locked with the file.

use std::sync::Arc;

use axum::{extract::{Path, State}, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use crate::Id;

use crate::auth::{AuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// SQL: the file `$1` is locked now (retention not over, or on legal hold).
/// The one statement of the rule for queries; [`guard_delete`] is the same rule
/// with the GOVERNANCE admin override and a message for people.
pub const LOCKED_NOW: &str = "EXISTS (SELECT 1 FROM object_locks l WHERE l.file_id = $1 AND (l.legal_hold OR l.retain_until > now()))";

#[derive(Debug, Clone, Default, Serialize, Deserialize, sqlx::FromRow)]
pub struct Lock {
    pub mode: Option<String>,
    pub retain_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub legal_hold: bool,
}

impl Lock {
    fn retained(&self) -> bool { self.retain_until.map(|t| t > Utc::now()).unwrap_or(false) }
    fn compliance(&self) -> bool { self.retained() && self.mode.as_deref() == Some("COMPLIANCE") }
}

pub async fn get(db: &PgPool, file_id: Id) -> Result<Lock, ApiError> {
    Ok(sqlx::query_as("SELECT mode, retain_until, legal_hold FROM object_locks WHERE file_id = $1")
        .bind(file_id).fetch_optional(db).await?.unwrap_or_default())
}

/// Refuse to delete (Trash or forever) a file that is locked or on legal hold.
pub async fn guard_delete(db: &PgPool, file_id: Id, user: &User) -> Result<(), ApiError> {
    let lock = get(db, file_id).await?;
    if lock.legal_hold {
        return Err(ApiError::Denied("This file is on legal hold and can't be deleted.".into()));
    }
    if lock.retained() && (lock.compliance() || user.role != "admin") {
        let until = lock.retain_until.map(|t| t.format("%-d %b %Y").to_string()).unwrap_or_default();
        return Err(ApiError::Denied(format!("This file is locked until {until} and can't be deleted.")));
    }
    Ok(())
}

/// Lock a new file for the policy's number of days, if it has one.
pub async fn apply_default(db: &PgPool, file_id: Id, policy: &crate::basket::Policy, user_id: Option<&str>) -> Result<(), ApiError> {
    let (Some(days), Some(mode)) = (policy.lock_days.filter(|d| *d > 0), policy.lock_mode.as_deref()) else { return Ok(()) };
    sqlx::query("INSERT INTO object_locks (file_id, mode, retain_until, set_by) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING")
        .bind(file_id).bind(mode).bind(Utc::now() + Duration::days(days as i64)).bind(user_id)
        .execute(db).await?;
    Ok(())
}

/// Change a file's lock. Rules: Manage on the file; a COMPLIANCE lock can only
/// be extended; shortening or removing a GOVERNANCE lock needs an admin.
pub async fn set(db: &PgPool, file_id: Id, user: &User, want: &Lock) -> Result<Lock, ApiError> {
    crate::acl::require_file(db, user, file_id, crate::acl::MANAGE).await?;
    if let Some(m) = want.mode.as_deref() {
        if m != "GOVERNANCE" && m != "COMPLIANCE" { return Err(ApiError::BadRequest("mode must be GOVERNANCE or COMPLIANCE".into())); }
        if want.retain_until.is_none() { return Err(ApiError::BadRequest("A lock needs a date to keep the file until.".into())); }
    }
    let cur = get(db, file_id).await?;
    let shorter = match (cur.retain_until, want.retain_until) {
        (Some(c), Some(w)) => w < c,
        (Some(_), None) => true,
        _ => false,
    } && cur.retained();
    if cur.compliance() && (shorter || want.mode.as_deref() != Some("COMPLIANCE")) {
        return Err(ApiError::Denied("A COMPLIANCE lock can only be extended, never shortened or removed.".into()));
    }
    if shorter && user.role != "admin" {
        return Err(ApiError::Denied("Only an admin can shorten or remove a lock.".into()));
    }
    sqlx::query(
        "INSERT INTO object_locks (file_id, mode, retain_until, legal_hold, set_by, updated_at) VALUES ($1,$2,$3,$4,$5,now())
         ON CONFLICT (file_id) DO UPDATE SET mode = EXCLUDED.mode, retain_until = EXCLUDED.retain_until,
           legal_hold = EXCLUDED.legal_hold, set_by = EXCLUDED.set_by, updated_at = now()",
    ).bind(file_id).bind(&want.mode).bind(want.retain_until).bind(want.legal_hold).bind(&user.id)
    .execute(db).await?;
    get(db, file_id).await
}

// ── REST: GET/PUT /api/files/:id/lock ──

pub async fn get_lock(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<Json<Lock>> {
    crate::acl::require_file(&s.db, &user.0, id, crate::acl::VIEW).await?;
    Ok(Json(get(&s.db, id).await?))
}

pub async fn put_lock(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>, Json(want): Json<Lock>) -> ApiResult<Json<Lock>> {
    let lock = set(&s.db, id, &user.0, &want).await?;
    let (name, sys): (String, String) = sqlx::query_as("SELECT name, system_id FROM files WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    crate::basket::log_activity(&s.db, &user.0, &describe(&lock), &name, "file", Some(id), &sys).await;
    Ok(Json(lock))
}

fn describe(l: &Lock) -> String {
    match (&l.mode, l.retain_until, l.legal_hold) {
        (_, _, true) => "put on legal hold".into(),
        (Some(m), Some(t), _) => format!("locked ({m}) until {}", t.format("%-d %b %Y")),
        _ => "lock removed".into(),
    }
}

//! Where each file came from: channel (web / api / connector), the connector,
//! and what the source system sent — system code, the person who pressed send
//! there, organisation, reference number, screen. Written once at upload and
//! never edited (the table refuses UPDATE).

use std::sync::Arc;
use axum::{extract::{Path, State}, Json};
use serde::Serialize;
use sqlx::PgPool;

use crate::{auth::AuthUser, error::ApiResult, AppState, Id};

/// What the caller said about the file's source. Empty fields are left out.
#[derive(Debug, Default, Clone)]
pub struct Extra {
    pub source_system: Option<String>,
    pub source_user: Option<String>,
    pub org_code: Option<String>,
    pub ref_no: Option<String>,
    pub screen: Option<String>,
    pub client_ip: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Origin {
    pub channel: String,
    pub connector_name: Option<String>,
    pub source_system: Option<String>,
    pub source_user: Option<String>,
    pub org_code: Option<String>,
    pub ref_no: Option<String>,
    pub screen: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

fn clip(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().chars().take(200).collect::<String>()).filter(|s| !s.is_empty())
}

/// Record the origin of a new file. A second call for the same file is ignored.
pub async fn record(db: &PgPool, file_id: &str, actor: Option<&str>, x: Extra) {
    let conn: Option<String> = match actor {
        Some(a) => sqlx::query_scalar("SELECT c.name FROM users u JOIN connectors c ON c.id = u.connector_id WHERE u.id = $1")
            .bind(a).fetch_optional(db).await.ok().flatten(),
        None => None,
    };
    let is_key: bool = match actor {
        Some(a) => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM api_keys WHERE user_id = $1)")
            .bind(a).fetch_one(db).await.unwrap_or(false),
        None => false,
    };
    let channel = if conn.is_some() { "connector" } else if is_key { "api" } else { "web" };
    let r = sqlx::query(
        "INSERT INTO file_origin (file_id, channel, connector_name, source_system, source_user, org_code, ref_no, screen, client_ip)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (file_id) DO NOTHING",
    ).bind(file_id).bind(channel).bind(conn)
     .bind(clip(x.source_system)).bind(clip(x.source_user)).bind(clip(x.org_code))
     .bind(clip(x.ref_no)).bind(clip(x.screen)).bind(clip(x.client_ip))
     .execute(db).await;
    if let Err(e) = r { tracing::warn!("file origin not recorded for {file_id}: {e}"); }
}

/// Origin fields a caller may send as headers on any upload.
pub fn from_headers(h: &axum::http::HeaderMap) -> Extra {
    let g = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
    Extra {
        source_system: g("x-filehub-source"),
        source_user: g("x-filehub-user"),
        org_code: g("x-filehub-org"),
        ref_no: g("x-filehub-ref"),
        screen: g("x-filehub-screen"),
        client_ip: g("x-forwarded-for").map(|v| v.split(',').next().unwrap_or("").trim().to_string()),
    }
}

/// GET /api/files/:id/origin — anyone who can see the file.
pub async fn get(State(s): State<Arc<AppState>>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<Json<Option<Origin>>> {
    crate::acl::require_file(&s.db, &user.0, id, crate::acl::VIEW).await.map_err(|_| crate::error::ApiError::NotFound)?;
    Ok(Json(sqlx::query_as("SELECT channel, connector_name, source_system, source_user, org_code, ref_no, screen, created_at FROM file_origin WHERE file_id = $1")
        .bind(id.to_string()).fetch_optional(&s.db).await?))
}

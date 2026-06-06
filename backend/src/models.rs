use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// System entities use TEXT/CUID2 PKs (see migrations 0001/0008).  Their IDs
// stay as `String` in Rust.  Transaction entities (files, activity, sessions,
// share_links, comments, notifications, workflows, file_versions, thumbnails)
// use UUIDv7 — `Uuid` in Rust.

#[derive(Debug, Serialize, FromRow)]
pub struct System {
    pub id: String,
    pub name: String,
    pub tone: String,
    pub bucket: String,
    pub status: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    /// 'shared' (everyone-visible) or 'personal' (per-user My Drive).
    pub system_type: String,
    /// Owner for 'personal' systems; NULL for shared.
    pub owner_user_id: Option<String>,
    /// System-level cap.  0 ⇒ inherit (no system cap).
    pub quota_bytes: i64,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Org {
    pub id: String,
    pub system_id: String,
    pub name: String,
    pub code: String,
    pub tier: String,
    pub owner: String,
    pub tags: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    /// Org-level cap.  0 ⇒ inherit (no org cap).
    pub quota_bytes: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct File {
    pub id: Uuid,
    pub name: String,
    pub file_type: String,
    pub size_bytes: i64,
    pub system_id: String,
    pub org_id: Option<String>,
    pub bucket: String,
    pub object_key: String,
    pub project: Option<String>,
    pub status: String,
    pub owner: String,
    pub tags: String,
    pub version: i64,
    pub metadata: String,
    pub etag: Option<String>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub folder_id: Option<String>,
    pub encrypted: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_by:  Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Folder {
    pub id: String,
    pub system_id: String,
    pub org_id: Option<String>,
    pub parent_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub owner: String,
    pub encrypted: bool,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_by: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, FromRow)]
pub struct ShareLink {
    pub id: Uuid,
    pub file_id: Uuid,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Activity {
    pub id: Uuid,
    pub actor: String,
    pub actor_tone: String,
    pub action: String,
    pub target: Option<String>,
    pub target_type: Option<String>,
    pub file_id: Option<Uuid>,
    pub system_id: Option<String>,
    pub org_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct View {
    pub id: String,
    pub name: String,
    pub layout: String,
    pub source: String,
    pub filters: String,
    pub group_by: Option<String>,
    pub sort_by: Option<String>,
    pub fields: String,
    pub pinned: bool,
    pub color: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Permission {
    pub id: Uuid,
    pub file_id: Option<Uuid>,
    pub org_id: Option<String>,
    pub principal: String,
    pub principal_type: String,
    pub role: String,
    pub external: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct FileVersion {
    pub id: Uuid,
    pub file_id: Uuid,
    pub version: i64,
    pub object_key: String,
    pub size_bytes: i64,
    pub etag: Option<String>,
    pub uploaded_by: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct DashboardStats {
    pub total_files: i64,
    pub total_size_bytes: i64,
    pub total_quota_bytes: i64,
    pub active_orgs: i64,
    pub total_orgs: i64,
    pub awaiting_review: i64,
    pub storage_by_system: Vec<SystemStorage>,
    pub connected_systems: Vec<ConnectedSystem>,
    /// Workspace branding read from the `workspace_config` k/v table — used
    /// by the settings page so admins can rename the workspace via SQL.
    pub workspace_name: String,
    pub workspace_display: String,
    /// Storage backend label from `Storage::backend_label()` (e.g.
    /// `"filesystem(/abs/path)"` or `"s3(bucket@endpoint)"`).
    pub storage_backend: String,
    pub encryption_enabled: bool,
}

#[derive(Debug, Serialize, FromRow)]
pub struct SystemStorage {
    pub system_id: String,
    pub name: String,
    pub tone: String,
    pub size_bytes: i64,
    pub file_count: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ConnectedSystem {
    pub id: String,
    pub name: String,
    pub tone: String,
    pub status: String,
    pub file_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct FilesQuery {
    pub system_id: Option<String>,
    pub org_id: Option<String>,
    pub status: Option<String>,
    pub project: Option<String>,
    pub owner: Option<String>,
    /// Sort field: name | size | status | owner | created | (default) modified.
    pub sort: Option<String>,
    /// asc | (default) desc.
    pub dir: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct OrgsQuery {
    pub system_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateView {
    pub name: String,
    pub layout: Option<String>,
    pub filters: Option<serde_json::Value>,
    pub group_by: Option<String>,
    pub sort_by: Option<String>,
    pub fields: Option<serde_json::Value>,
    pub color: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub system_id: Option<String>,
    pub file_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

/// Distinguish "key absent" from "explicit null".
pub fn deserialize_some<'de, T, D>(d: D) -> Result<Option<T>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
pub struct PatchFile {
    pub name: Option<String>,
    pub system_id: Option<String>,
    pub org_id: Option<String>,
    pub project: Option<String>,
    pub status: Option<String>,
    pub owner: Option<String>,
    pub tags: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub folder_id: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFolder {
    pub name: String,
    pub system_id: String,
    pub org_id: Option<String>,
    pub parent_id: Option<String>,
    pub color: Option<String>,
    pub owner: Option<String>,
    pub encrypted: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct FoldersQuery {
    pub system_id: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateShareLink {
    pub expires_in_days: Option<i64>,
    pub created_by: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ShareResponse {
    pub token: String,
    pub url: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub file: File,
}

#[derive(Debug, Deserialize)]
pub struct ReportTimeQuery {
    pub bucket: Option<String>,
    pub limit:  Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CategoryReportRow {
    pub category: String,
    pub file_count: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct TimeReportRow {
    pub bucket: DateTime<Utc>,
    pub file_count: i64,
    pub size_bytes: i64,
}

// -----------------------------------------------------------------------------
// Phase H — system create / patch payloads
// -----------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct CreateSystem {
    pub name: String,
    pub bucket: Option<String>,           // auto-generated from name if omitted
    pub tone: Option<String>,
    pub description: Option<String>,
    pub quota_bytes: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PatchSystem {
    pub name: Option<String>,
    pub tone: Option<String>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub quota_bytes: Option<i64>,
}

// -----------------------------------------------------------------------------
// Phase J — org / user quota patch
// -----------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct PatchOrgQuota {
    pub quota_bytes: i64,
}

#[derive(Debug, Deserialize)]
pub struct PatchUserQuota {
    pub quota_bytes: i64,
}

// -----------------------------------------------------------------------------
// Phase K — rotation policies
// -----------------------------------------------------------------------------
#[derive(Debug, Serialize, FromRow)]
pub struct RotationPolicy {
    pub id: String,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub keep_last_n_versions: i32,
    pub archive_after_days: i32,
    pub delete_after_days: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertRotationPolicy {
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub keep_last_n_versions: i32,
    pub archive_after_days: i32,
    pub delete_after_days: i32,
}

#[derive(Debug, Serialize, FromRow)]
pub struct RotationRun {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub versions_pruned: i32,
    pub files_archived: i32,
    pub files_hard_deleted: i32,
    pub triggered_by: Option<String>,
    pub error: Option<String>,
}

// -----------------------------------------------------------------------------
// Phase J — quota usage breakdown returned by GET /api/quota
// -----------------------------------------------------------------------------
#[derive(Debug, Serialize)]
pub struct QuotaScope {
    pub scope: &'static str,                // "workspace" | "system" | "org" | "user"
    pub id: Option<String>,                 // None for workspace
    pub limit_bytes: i64,                   // 0 = unlimited at this scope
    pub used_bytes: i64,
}

#[derive(Debug, Serialize)]
pub struct QuotaReport {
    pub scopes: Vec<QuotaScope>,
}

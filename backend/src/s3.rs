//! S3-compatible endpoint so systems that write to MinIO/S3 today can write to
//! FileHub by changing only the endpoint and keys (path-style addressing).
//!
//! Enabled with `S3_PORT` (e.g. 9000): a second listener serves the S3 API at
//! the root, like MinIO. Bucket = a basket's storage key (`systems.bucket`).
//! Object key `a/b/c.pdf` = folders `a` › `b` and file `c.pdf` in that basket;
//! folders are created on PUT. Writing an existing key adds a new version.
//! `x-amz-meta-<field>` headers become basket field values
//! (`x-amz-meta-category` = category code).
//!
//! Auth: AWS Signature V4 (header or presigned query). Each access key belongs
//! to a FileHub user, so every call goes through the same View/Edit checks.
//!
//! Supported: ListBuckets, HeadBucket, GetBucketLocation, ListObjectsV2 (and V1
//! basics), PutObject (incl. aws-chunked), CopyObject, GetObject (Range),
//! HeadObject, DeleteObject, DeleteObjects, Create/UploadPart/Complete/Abort
//! multipart upload. Not supported (501): ACLs, bucket policy, tagging,
//! lifecycle, versioning APIs, object lock.

use std::collections::BTreeMap;
use std::sync::Arc;

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::{DateTime, NaiveDateTime, Utc};
use futures::StreamExt;
use hmac::{Hmac, Mac};
use md5::Md5;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use crate::Id;

use crate::auth::{AuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::handlers::{UploadFields, FILE_COLS};
use crate::models::File;
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

// -----------------------------------------------------------------------------
// Errors as S3 XML
// -----------------------------------------------------------------------------

pub struct S3Err { pub(crate) status: StatusCode, pub(crate) code: &'static str, pub(crate) msg: String }
fn s3e(status: StatusCode, code: &'static str, msg: impl Into<String>) -> S3Err { S3Err { status, code, msg: msg.into() } }
impl IntoResponse for S3Err {
    fn into_response(self) -> Response {
        let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{}</Code><Message>{}</Message></Error>", self.code, xml(&self.msg));
        (self.status, [(header::CONTENT_TYPE, "application/xml")], body).into_response()
    }
}
impl From<ApiError> for S3Err {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::NotFound => s3e(StatusCode::NOT_FOUND, "NoSuchKey", "The specified key does not exist."),
            ApiError::Forbidden => s3e(StatusCode::FORBIDDEN, "AccessDenied", "Access Denied"),
            ApiError::Unauthorized => s3e(StatusCode::FORBIDDEN, "AccessDenied", "Access Denied"),
            ApiError::BadRequest(m) => s3e(StatusCode::BAD_REQUEST, "InvalidRequest", m),
            ApiError::PayloadTooLarge(m) => s3e(StatusCode::BAD_REQUEST, "EntityTooLarge", m),
            ApiError::TooManyRequests(m) | ApiError::Busy(m) => s3e(StatusCode::SERVICE_UNAVAILABLE, "SlowDown", m),
            ApiError::Denied(m) => s3e(StatusCode::FORBIDDEN, "AccessDenied", m),
            ApiError::Timeout(m) => s3e(StatusCode::BAD_REQUEST, "RequestTimeout", m),
            ApiError::Conflict(m) => s3e(StatusCode::CONFLICT, "OperationAborted", m),
            ApiError::ContentLocked(m) => s3e(StatusCode::CONFLICT, "InvalidObjectState", m),
            other => { tracing::error!("s3: {other}"); s3e(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", "We encountered an internal error. Please try again.") }
        }
    }
}
impl From<sqlx::Error> for S3Err { fn from(e: sqlx::Error) -> Self { ApiError::Db(e).into() } }
impl From<anyhow::Error> for S3Err { fn from(e: anyhow::Error) -> Self { ApiError::Other(e).into() } }
impl From<std::io::Error> for S3Err { fn from(e: std::io::Error) -> Self { ApiError::Other(e.into()).into() } }
type S3Result<T> = Result<T, S3Err>;

/// The S3 XML namespace attribute every response root carries.
const XMLNS: &str = "xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"";

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}
fn xml_resp(status: StatusCode, body: String) -> Response {
    (status, [(header::CONTENT_TYPE, "application/xml")], format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{body}")).into_response()
}
fn iso(t: DateTime<Utc>) -> String { t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string() }

// -----------------------------------------------------------------------------
// Credentials (secret stored AES-GCM encrypted; SigV4 needs the server to know it)
// -----------------------------------------------------------------------------

fn cred_cipher() -> Option<Aes256Gcm> {
    let material = std::env::var("S3_CRED_KEY").ok()
        .or_else(|| std::env::var("STORAGE_ENC_KEY").ok())
        .or_else(|| std::env::var("WOPI_SECRET").ok())
        .filter(|v| !v.trim().is_empty())?;
    let k = Sha256::digest(format!("filehub-s3-credentials:{material}").as_bytes());
    Some(Aes256Gcm::new_from_slice(&k).ok()?)
}
fn seal(secret: &str) -> Result<String, ApiError> {
    let c = cred_cipher().ok_or_else(|| ApiError::BadRequest("Set S3_CRED_KEY (or STORAGE_ENC_KEY) on the server before creating S3 keys.".into()))?;
    let mut n = [0u8; 12];
    { rand::Rng::fill(&mut rand::thread_rng(), &mut n[..]); }
    let ct = c.encrypt(Nonce::from_slice(&n), secret.as_bytes()).map_err(|_| ApiError::Other(anyhow::anyhow!("seal")))?;
    let mut out = n.to_vec(); out.extend(ct);
    Ok(B64.encode(out))
}
fn open(sealed: &str) -> Option<String> {
    let c = cred_cipher()?;
    let raw = B64.decode(sealed).ok()?;
    if raw.len() < 13 { return None; }
    let pt = c.decrypt(Nonce::from_slice(&raw[..12]), &raw[12..]).ok()?;
    String::from_utf8(pt).ok()
}

#[derive(Debug, Serialize, FromRow)]
pub struct CredRow {
    access_key_id: String,
    label: String,
    user_id: String,
    user_name: Option<String>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

/// The S3 feature is on only where the S3 port is opened (S3_PORT).
pub fn enabled() -> bool {
    std::env::var("S3_PORT").ok().and_then(|p| p.parse::<u16>().ok()).is_some()
}
fn require_enabled() -> ApiResult<()> { if enabled() { Ok(()) } else { Err(ApiError::NotFound) } }

pub async fn list_credentials(State(s): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Vec<CredRow>>> {
    require_enabled()?;
    crate::auth::require_role(&user.0, &["admin"])?;
    Ok(Json(sqlx::query_as(
        "SELECT c.access_key_id, c.label, c.user_id, u.display_name AS user_name, c.created_at, c.last_used_at
           FROM s3_credentials c LEFT JOIN users u ON u.id = c.user_id WHERE c.revoked_at IS NULL ORDER BY c.created_at DESC",
    ).fetch_all(&s.db).await?))
}

#[derive(Debug, Deserialize)]
pub struct NewCred { pub label: String, pub user_id: Option<String> }

/// Create an access key for a user (default: the caller). The secret is shown once.
pub async fn create_credential(State(s): State<Arc<AppState>>, user: AuthUser, Json(b): Json<NewCred>) -> ApiResult<Json<serde_json::Value>> {
    require_enabled()?;
    crate::auth::require_role(&user.0, &["admin"])?;
    let (ak, sk) = new_key_pair();
    let label = b.label.trim();
    if label.is_empty() || label.len() > 120 { return Err(ApiError::BadRequest("Give the key a name (1–120 characters), e.g. the system that will use it.".into())); }
    let uid = b.user_id.unwrap_or_else(|| user.0.id.clone());
    let ok: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE id = $1 AND status = 'active'").bind(&uid).fetch_optional(&s.db).await?;
    if ok.is_none() { return Err(ApiError::BadRequest("Unknown user.".into())); }
    sqlx::query("INSERT INTO s3_credentials (access_key_id, secret_sealed, label, user_id, created_by) VALUES ($1,$2,$3,$4,$5)")
        .bind(&ak).bind(seal(&sk)?).bind(label).bind(&uid).bind(&user.0.id).execute(&s.db).await?;
    Ok(Json(serde_json::json!({ "access_key_id": ak, "secret_access_key": sk, "label": label, "user_id": uid })))
}

fn new_key_pair() -> (String, String) {
    const AK: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    const SK: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let mut pick = |set: &[u8], n: usize| -> String { (0..n).map(|_| set[rand::Rng::gen_range(&mut rng, 0..set.len())] as char).collect() };
    (format!("FH{}", pick(AK, 18)), pick(SK, 40))
}

/// Where systems should point their S3 client (shown on the S3 keys screen).
pub async fn endpoint_info(_user: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "enabled": std::env::var("S3_PORT").ok().filter(|v| !v.is_empty()).is_some(),
        "port": std::env::var("S3_PORT").ok(),
        "public_url": std::env::var("S3_PUBLIC_URL").ok(),
    }))
}

pub async fn revoke_credential(State(s): State<Arc<AppState>>, user: AuthUser, Path(ak): Path<String>) -> ApiResult<StatusCode> {
    require_enabled()?;
    crate::auth::require_role(&user.0, &["admin"])?;
    let r = sqlx::query("UPDATE s3_credentials SET revoked_at = now() WHERE access_key_id = $1 AND revoked_at IS NULL").bind(&ak).execute(&s.db).await?;
    if r.rows_affected() == 0 { return Err(ApiError::NotFound); }
    Ok(StatusCode::NO_CONTENT)
}

// -----------------------------------------------------------------------------
// SigV4
// -----------------------------------------------------------------------------

fn aws_enc(s: &str, slash: bool) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b'/' if !slash => o.push('/'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}
fn pct_dec(s: &str) -> String {
    urlencoding::decode(&s.replace('+', "%20")).map(|c| c.into_owned()).unwrap_or_else(|_| s.to_string())
}
fn hmac(key: &[u8], msg: &str) -> Vec<u8> {
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).expect("hmac");
    m.update(msg.as_bytes());
    m.finalize().into_bytes().to_vec()
}

struct Auth { user: User, payload_hash: String }

async fn authenticate(s: &AppState, method: &Method, raw_path: &str, raw_query: &str, headers: &HeaderMap) -> S3Result<Auth> {
    let q: Vec<(String, String)> = raw_query.split('&').filter(|p| !p.is_empty())
        .map(|p| { let (k, v) = p.split_once('=').unwrap_or((p, "")); (pct_dec(k), pct_dec(v)) }).collect();
    let qget = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone());
    let presigned = qget("X-Amz-Algorithm").is_some();
    // Legacy SigV2 presigned URL (?AWSAccessKeyId=&Expires=&Signature=), still
    // what several SDKs produce by default for GET links.
    if !presigned && qget("AWSAccessKeyId").is_some() && qget("Signature").is_some() {
        return sigv2_query(s, method, raw_path, &qget("AWSAccessKeyId").unwrap(), &qget("Expires").unwrap_or_default(), &qget("Signature").unwrap()).await;
    }

    let (cred, signed_headers, signature, amz_date, payload_hash) = if presigned {
        if qget("X-Amz-Algorithm").as_deref() != Some("AWS4-HMAC-SHA256") { return Err(s3e(StatusCode::BAD_REQUEST, "AuthorizationQueryParametersError", "Only AWS4-HMAC-SHA256 is supported.")); }
        let date = qget("X-Amz-Date").unwrap_or_default();
        let exp: i64 = qget("X-Amz-Expires").and_then(|v| v.parse().ok()).unwrap_or(0);
        let t = NaiveDateTime::parse_from_str(&date, "%Y%m%dT%H%M%SZ").map_err(|_| s3e(StatusCode::FORBIDDEN, "AccessDenied", "Bad X-Amz-Date."))?.and_utc();
        if exp <= 0 || exp > 604800 || Utc::now() > t + chrono::Duration::seconds(exp) {
            return Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "Request has expired."));
        }
        (qget("X-Amz-Credential").unwrap_or_default(), qget("X-Amz-SignedHeaders").unwrap_or_default(),
         qget("X-Amz-Signature").unwrap_or_default(), date, "UNSIGNED-PAYLOAD".to_string())
    } else {
        let a = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
            .ok_or_else(|| s3e(StatusCode::FORBIDDEN, "AccessDenied", "Anonymous access is not allowed."))?;
        let rest = a.strip_prefix("AWS4-HMAC-SHA256").ok_or_else(|| s3e(StatusCode::BAD_REQUEST, "InvalidArgument", "Only AWS Signature Version 4 is supported."))?;
        let mut parts: BTreeMap<String, String> = BTreeMap::new();
        for p in rest.split(',') { if let Some((k, v)) = p.trim().split_once('=') { parts.insert(k.trim().to_string(), v.trim().to_string()); } }
        let date = headers.get("x-amz-date").and_then(|v| v.to_str().ok()).map(str::to_string)
            .or_else(|| headers.get(header::DATE).and_then(|v| v.to_str().ok()).map(str::to_string)).unwrap_or_default();
        let t = NaiveDateTime::parse_from_str(&date, "%Y%m%dT%H%M%SZ").map_err(|_| s3e(StatusCode::FORBIDDEN, "AccessDenied", "Missing or bad x-amz-date."))?.and_utc();
        if (Utc::now() - t).num_seconds().abs() > 900 { return Err(s3e(StatusCode::FORBIDDEN, "RequestTimeTooSkewed", "The difference between the request time and the server's time is too large.")); }
        let ph = headers.get("x-amz-content-sha256").and_then(|v| v.to_str().ok()).unwrap_or("UNSIGNED-PAYLOAD").to_string();
        (parts.get("Credential").cloned().unwrap_or_default(), parts.get("SignedHeaders").cloned().unwrap_or_default(),
         parts.get("Signature").cloned().unwrap_or_default(), date, ph)
    };

    let mut cparts = cred.split('/');
    let ak = cparts.next().unwrap_or("").to_string();
    let scope_rest: Vec<&str> = cparts.collect();
    if scope_rest.len() != 4 || scope_rest[2] != "s3" || scope_rest[3] != "aws4_request" {
        return Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "Bad credential scope."));
    }
    let row: Option<(String, String)> = sqlx::query_as("SELECT secret_sealed, user_id FROM s3_credentials WHERE access_key_id = $1 AND revoked_at IS NULL")
        .bind(&ak).fetch_optional(&s.db).await?;
    let Some((sealed, uid)) = row else { return Err(s3e(StatusCode::FORBIDDEN, "InvalidAccessKeyId", "The access key ID you provided does not exist in our records.")); };
    let secret = open(&sealed).ok_or_else(|| s3e(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", "Server key for S3 credentials changed; create a new access key."))?;

    let mut cq: Vec<(String, String)> = q.iter().filter(|(k, _)| k != "X-Amz-Signature")
        .map(|(k, v)| (aws_enc(k, true), aws_enc(v, true))).collect();
    cq.sort();
    let canon_q = cq.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    let mut canon_h = String::new();
    // The host must be signed, or a captured request could be replayed elsewhere.
    if !signed_headers.split(';').any(|h| h == "host") {
        return Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "The host header must be signed."));
    }
    for h in signed_headers.split(';') {
        let v = headers.get_all(h).iter().filter_map(|x| x.to_str().ok()).map(|x| x.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(",");
        canon_h.push_str(&format!("{h}:{v}\n"));
    }
    let canon_path = if raw_path.is_empty() { "/".to_string() } else { raw_path.to_string() };
    let canonical = format!("{}\n{}\n{}\n{}\n{}\n{}", method.as_str(), canon_path, canon_q, canon_h, signed_headers, payload_hash);
    let scope = scope_rest.join("/");
    let sts = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", hex::encode(Sha256::digest(canonical.as_bytes())));
    let k = hmac(&hmac(&hmac(&hmac(format!("AWS4{secret}").as_bytes(), scope_rest[0]), scope_rest[1]), "s3"), "aws4_request");
    let want = hex::encode(hmac(&k, &sts));
    if !crate::config::constant_eq(want.as_bytes(), signature.as_bytes()) {
        tracing::debug!(canonical = %canonical, "s3 signature mismatch");
        return Err(s3e(StatusCode::FORBIDDEN, "SignatureDoesNotMatch", "The request signature we calculated does not match the signature you provided."));
    }
    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1 AND status = 'active'").bind(&uid).fetch_optional(&s.db).await?
        .ok_or_else(|| s3e(StatusCode::FORBIDDEN, "AccessDenied", "The user behind this key is disabled."))?;
    let _ = sqlx::query("UPDATE s3_credentials SET last_used_at = now() WHERE access_key_id = $1").bind(&ak).execute(&s.db).await;
    Ok(Auth { user, payload_hash })
}
async fn sigv2_query(s: &AppState, method: &Method, raw_path: &str, ak: &str, expires: &str, sig: &str) -> S3Result<Auth> {
    let exp: i64 = expires.parse().map_err(|_| s3e(StatusCode::FORBIDDEN, "AccessDenied", "Bad Expires."))?;
    if Utc::now().timestamp() > exp || exp - Utc::now().timestamp() > 604800 {
        return Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "Request has expired."));
    }
    let row: Option<(String, String)> = sqlx::query_as("SELECT secret_sealed, user_id FROM s3_credentials WHERE access_key_id = $1 AND revoked_at IS NULL")
        .bind(ak).fetch_optional(&s.db).await?;
    let Some((sealed, uid)) = row else { return Err(s3e(StatusCode::FORBIDDEN, "InvalidAccessKeyId", "The access key ID you provided does not exist in our records.")) };
    let secret = open(&sealed).ok_or_else(|| s3e(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", "Server key for S3 credentials changed."))?;
    // StringToSign = VERB \n Content-MD5 \n Content-Type \n Expires \n CanonicalizedResource
    let sts = format!("{}\n\n\n{}\n{}", method.as_str(), exp, raw_path);
    let mut m = <hmac::Hmac<sha1::Sha1> as Mac>::new_from_slice(secret.as_bytes()).expect("hmac");
    m.update(sts.as_bytes());
    let want = B64.encode(m.finalize().into_bytes());
    if !crate::config::constant_eq(want.as_bytes(), sig.as_bytes()) {
        return Err(s3e(StatusCode::FORBIDDEN, "SignatureDoesNotMatch", "The request signature we calculated does not match the signature you provided."));
    }
    if *method != Method::GET && *method != Method::HEAD {
        return Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "Signature V2 links are accepted for reading only."));
    }
    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1 AND status = 'active'").bind(&uid).fetch_optional(&s.db).await?
        .ok_or_else(|| s3e(StatusCode::FORBIDDEN, "AccessDenied", "The user behind this key is disabled."))?;
    Ok(Auth { user, payload_hash: "UNSIGNED-PAYLOAD".into() })
}

// -----------------------------------------------------------------------------
// Basket / key resolution
// -----------------------------------------------------------------------------

async fn basket_of(s: &AppState, user: &User, bucket: &str) -> S3Result<(String, String)> {
    let row: Option<(String, String)> = sqlx::query_as("SELECT id, name FROM systems WHERE bucket = $1 AND deleted_at IS NULL")
        .bind(bucket).fetch_optional(&s.db).await?;
    let Some((id, name)) = row else { return Err(s3e(StatusCode::NOT_FOUND, "NoSuchBucket", "The specified bucket does not exist.")) };
    if crate::auth::ensure_system_access(&s.db, user, &id).await.is_err() {
        return Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "Access Denied"));
    }
    Ok((id, name))
}

fn split_key(key: &str) -> (Vec<String>, String) {
    let parts: Vec<String> = key.split('/').map(str::to_string).collect();
    let name = parts.last().cloned().unwrap_or_default();
    let dirs = parts[..parts.len().saturating_sub(1)].iter().filter(|p| !p.is_empty()).cloned().collect();
    (dirs, name)
}

/// Folder id for a path, creating missing folders when `create`.
async fn folder_for(s: &AppState, user: &User, sys: &str, dirs: &[String], create: bool) -> S3Result<Option<Option<String>>> {
    let mut parent: Option<String> = None;
    if dirs.iter().any(|d| !crate::basket::valid_folder_name(d)) {
        return Err(s3e(StatusCode::BAD_REQUEST, "InvalidArgument", "Object key has an empty, \".\" or \"..\" path segment."));
    }
    for d in dirs {
        let found: Option<String> = match &parent {
            None => sqlx::query_scalar("SELECT id FROM folders WHERE system_id = $1 AND parent_id IS NULL AND name = $2 AND deleted_at IS NULL ORDER BY created_at LIMIT 1").bind(sys).bind(d).fetch_optional(&s.db).await?,
            Some(p) => sqlx::query_scalar("SELECT id FROM folders WHERE system_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL ORDER BY created_at LIMIT 1").bind(sys).bind(p).bind(d).fetch_optional(&s.db).await?,
        };
        parent = Some(match found {
            Some(id) => id,
            None if create => {
                crate::acl::require_folder(&s.db, user, sys, parent.as_deref(), crate::acl::EDIT).await
                    .map_err(|_| s3e(StatusCode::FORBIDDEN, "AccessDenied", "Access Denied"))?;
                let id = crate::new_id();
                sqlx::query("INSERT INTO folders (id, system_id, parent_id, name, owner, encrypted, created_by) VALUES ($1,$2,$3,$4,$5,false,$6)")
                    .bind(&id).bind(sys).bind(&parent).bind(d).bind(&user.display_name).bind(&user.id).execute(&s.db).await?;
                id
            }
            None => return Ok(None),
        });
    }
    Ok(Some(parent))
}

pub(crate) async fn find_object(s: &AppState, user: &User, sys: &str, key: &str) -> S3Result<File> {
    let (dirs, name) = split_key(key);
    let Some(folder) = folder_for(s, user, sys, &dirs, false).await? else { return Err(ApiError::NotFound.into()) };
    let f: Option<File> = match &folder {
        None => sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE system_id = $1 AND folder_id IS NULL AND name = $2 AND deleted_at IS NULL ORDER BY modified_at DESC LIMIT 1"))
            .bind(sys).bind(&name).fetch_optional(&s.db).await?,
        Some(fid) => sqlx::query_as(&format!("SELECT {FILE_COLS} FROM files WHERE system_id = $1 AND folder_id = $2 AND name = $3 AND deleted_at IS NULL ORDER BY modified_at DESC LIMIT 1"))
            .bind(sys).bind(fid).bind(&name).fetch_optional(&s.db).await?,
    };
    let f = f.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, user, f.id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    Ok(f)
}

// -----------------------------------------------------------------------------
// Body handling: temp file, payload hash check, aws-chunked decoding
// -----------------------------------------------------------------------------

/// Where S3 bodies and multipart parts wait until complete. Must be one shared
/// volume when running several pods (parts of one upload can hit different pods).
fn s3_tmp() -> std::path::PathBuf {
    crate::storage::spool_root().join(".s3")
}

/// Write the request body to a temp file (decoded), verifying the payload hash.
async fn spool(body: Body, payload_hash: &str, chunked: bool) -> S3Result<(std::path::PathBuf, u64, String)> {
    let dir = s3_tmp();
    tokio::fs::create_dir_all(&dir).await?;
    let raw_path = dir.join(format!("raw-{}", crate::Id::new()));
    let mut f = tokio::fs::File::create(&raw_path).await?;
    let mut sha = Sha256::new();
    let mut n: u64 = 0;
    let max = crate::handlers::upload_max_bytes();
    let mut stream = body.into_data_stream();
    while let Some(c) = stream.next().await {
        let c = c.map_err(|e| s3e(StatusCode::BAD_REQUEST, "IncompleteBody", e.to_string()))?;
        n += c.len() as u64;
        if n > max + 1024 * 1024 { let _ = tokio::fs::remove_file(&raw_path).await; return Err(s3e(StatusCode::BAD_REQUEST, "EntityTooLarge", "Your proposed upload exceeds the maximum allowed size.")); }
        sha.update(&c);
        f.write_all(&c).await?;
    }
    f.flush().await?;
    drop(f);
    if payload_hash.len() == 64 && payload_hash.chars().all(|c| c.is_ascii_hexdigit()) && hex::encode(sha.finalize()) != payload_hash.to_lowercase() {
        let _ = tokio::fs::remove_file(&raw_path).await;
        return Err(s3e(StatusCode::BAD_REQUEST, "XAmzContentSHA256Mismatch", "The provided 'x-amz-content-sha256' header does not match what was computed."));
    }
    let path = if chunked {
        let out = dir.join(format!("dec-{}", crate::Id::new()));
        decode_chunked(&raw_path, &out).await?;
        let _ = tokio::fs::remove_file(&raw_path).await;
        out
    } else { raw_path };
    // MD5 of the decoded bytes = the ETag S3 clients compare.
    let mut md5 = Md5::new();
    let mut size = 0u64;
    let mut r = tokio::fs::File::open(&path).await?;
    let mut buf = vec![0u8; 256 * 1024];
    loop { let k = r.read(&mut buf).await?; if k == 0 { break; } size += k as u64; md5.update(&buf[..k]); }
    Ok((path, size, hex::encode(md5.finalize())))
}

/// aws-chunked: `<hex-size>[;chunk-signature=…]\r\n<data>\r\n` … `0…\r\n` [trailers]\r\n
async fn decode_chunked(src: &std::path::Path, dst: &std::path::Path) -> S3Result<()> {
    let mut r = BufReader::new(tokio::fs::File::open(src).await?);
    let mut w = tokio::fs::File::create(dst).await?;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line).await? == 0 { break; }
        let size_hex = line.trim_end().split(';').next().unwrap_or("").trim();
        if size_hex.is_empty() { continue; }
        let size = u64::from_str_radix(size_hex, 16).map_err(|_| s3e(StatusCode::BAD_REQUEST, "InvalidRequest", "Malformed aws-chunked body."))?;
        if size == 0 { break; }
        let mut left = size;
        let mut buf = vec![0u8; 64 * 1024];
        while left > 0 {
            let k = r.read(&mut buf[..(left.min(64 * 1024)) as usize]).await?;
            if k == 0 { return Err(s3e(StatusCode::BAD_REQUEST, "IncompleteBody", "Body ended inside a chunk.")); }
            w.write_all(&buf[..k]).await?;
            left -= k as u64;
        }
        let mut crlf = [0u8; 2];
        let _ = r.read_exact(&mut crlf).await;
    }
    w.flush().await?;
    Ok(())
}

async fn stage_file(s: &AppState, path: &std::path::Path) -> S3Result<crate::storage::Staged> {
    let f = tokio::fs::File::open(path).await?;
    let st = s.storage.stage_stream(tokio_util::io::ReaderStream::with_capacity(f, 256 * 1024)).await?;
    let _ = tokio::fs::remove_file(path).await;
    Ok(st)
}

fn meta_from(headers: &HeaderMap) -> (serde_json::Map<String, serde_json::Value>, Option<String>) {
    let mut m = serde_json::Map::new();
    let mut cat = None;
    for (k, v) in headers {
        if let Some(name) = k.as_str().strip_prefix("x-amz-meta-") {
            let val = v.to_str().map(|x| pct_dec(x)).unwrap_or_default();
            if name == "category" { cat = Some(val); continue; }
            let key = name.replace('-', "_");
            if key.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) { m.insert(key, serde_json::Value::String(val)); }
        }
    }
    (m, cat)
}

/// Store a decoded body under `key`: new file, or a new version when the key exists.
/// Store a spooled body at `key`: a new file, or the next version of the file
/// already there. Returns (etag, version id).
pub(crate) async fn put_object_from(s: &Arc<AppState>, user: &User, sys: &str, key: &str, path: std::path::PathBuf, headers: &HeaderMap) -> S3Result<(String, i64)> {
    let (dirs, name) = split_key(key);
    if name.is_empty() {
        // "folder/" marker objects: just make sure the folders exist.
        let _ = tokio::fs::remove_file(&path).await;
        folder_for(s, user, sys, &dirs, true).await?;
        return Ok(("d41d8cd98f00b204e9800998ecf8427e".into(), 1));
    }
    let folder = folder_for(s, user, sys, &dirs, true).await?.flatten();
    let (meta, cat) = meta_from(headers);
    crate::auth::upload_rate_limit_keyed(&s.db, &format!("s3:{}", user.id), 600).await?;
    let staged = stage_file(s, &path).await?;

    if let Ok(existing) = find_object(s, user, sys, key).await {
        // Same key = next version, through the same path as the web app
        // (Edit, unlocked status, policy, quota, streamed, preview refreshed).
        let file = crate::handlers::commit_version(s, user, existing.id, staged, None, None, Some("S3 PUT".into())).await
            .map_err(|e| match e {
                ApiError::ContentLocked(m) => s3e(StatusCode::CONFLICT, "InvalidObjectState", m),
                other => other.into(),
            })?;
        if !meta.is_empty() || cat.is_some() {
            let mut merged: serde_json::Value = serde_json::from_str(&existing.metadata).unwrap_or(serde_json::json!({}));
            if let Some(o) = merged.as_object_mut() { for (k, v) in meta { o.insert(k, v); } }
            let checked = crate::basket::validate_fields(&s.db, sys, &merged, false).await?;
            sqlx::query("UPDATE files SET metadata = $1, category_code = COALESCE($2, category_code) WHERE id = $3")
                .bind(checked.to_string()).bind(cat).bind(existing.id).execute(&s.db).await?;
        }
        return Ok((file.etag.unwrap_or_default(), file.version));
    }

    let f = UploadFields {
        name: Some(name),
        system_id: Some(sys.to_string()),
        folder_id: folder,
        content_type: headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string),
        fields: if meta.is_empty() { None } else { Some(serde_json::Value::Object(meta).to_string()) },
        category_code: cat,
        skip_required: true,
        staged: Some(staged),
        ..Default::default()
    };
    let file = crate::handlers::persist_upload_for(s, Some(user.id.as_str()), f, Some(user)).await?;
    sqlx::query("UPDATE activity SET action = 'uploaded via S3' WHERE file_id = $1 AND action = 'uploaded'").bind(file.id).execute(&s.db).await?;
    Ok((file.etag.unwrap_or_default(), file.version))
}

// -----------------------------------------------------------------------------
// Router entry
// -----------------------------------------------------------------------------

pub fn router(state: Arc<AppState>) -> axum::Router {
    axum::Router::new()
        .route("/", axum::routing::any(entry))
        .route("/*path", axum::routing::any(entry))
        .layer(axum::extract::DefaultBodyLimit::disable())
        .with_state(state)
}

async fn entry(State(s): State<Arc<AppState>>, req: Request) -> Response {
    match dispatch(s, req).await { Ok(r) => r, Err(e) => e.into_response() }
}

async fn dispatch(s: Arc<AppState>, req: Request) -> S3Result<Response> {
    let (parts, body) = req.into_parts();
    let raw_path = parts.uri.path().to_string();
    let raw_query = parts.uri.query().unwrap_or("").to_string();
    let method = parts.method.clone();
    let headers = parts.headers.clone();
    let auth = authenticate(&s, &method, &raw_path, &raw_query, &headers).await?;
    let user = auth.user;
    let q: BTreeMap<String, String> = raw_query.split('&').filter(|p| !p.is_empty())
        .map(|p| { let (k, v) = p.split_once('=').unwrap_or((p, "")); (pct_dec(k), pct_dec(v)) }).collect();

    let path = pct_dec(raw_path.trim_start_matches('/'));
    let (bucket, key) = match path.split_once('/') { Some((b, k)) => (b.to_string(), k.to_string()), None => (path.clone(), String::new()) };

    if bucket.is_empty() {
        if method != Method::GET { return Err(s3e(StatusCode::METHOD_NOT_ALLOWED, "MethodNotAllowed", "The specified method is not allowed against this resource.")); }
        return list_buckets(&s, &user).await;
    }
    for unsupported in ["acl", "policy", "lifecycle", "versioning", "cors", "encryption", "object-lock", "replication", "notification", "website", "logging"] {
        let readable = method == Method::GET && matches!(unsupported, "versioning" | "object-lock");
        if q.contains_key(unsupported) && !readable {
            return Err(s3e(StatusCode::NOT_IMPLEMENTED, "NotImplemented", format!("FileHub doesn't support the ?{unsupported} API. Manage this in the FileHub console.")));
        }
    }

    if key.is_empty() {
        return match method {
            Method::HEAD => { basket_of(&s, &user, &bucket).await?; Ok(StatusCode::OK.into_response()) }
            Method::GET if q.contains_key("location") => { basket_of(&s, &user, &bucket).await?; Ok(xml_resp(StatusCode::OK, format!("<LocationConstraint {XMLNS}>us-east-1</LocationConstraint>"))) }
            Method::GET if q.contains_key("versioning") => { basket_of(&s, &user, &bucket).await?; Ok(xml_resp(StatusCode::OK, format!("<VersioningConfiguration {XMLNS}><Status>Enabled</Status></VersioningConfiguration>"))) }
            Method::GET if q.contains_key("object-lock") => { let (sys, _) = basket_of(&s, &user, &bucket).await?; bucket_lock_config(&s, &sys).await }
            Method::GET if q.contains_key("versions") => list_versions(&s, &user, &bucket, &q).await,
            Method::GET if q.contains_key("uploads") => { let (sys, _) = basket_of(&s, &user, &bucket).await?; list_uploads(&s, &user, &sys, &bucket, &q).await }
            Method::GET => list_objects(&s, &user, &bucket, &q).await,
            Method::POST if q.contains_key("delete") => {
                let (sys, _) = basket_of(&s, &user, &bucket).await?;
                let b = axum::body::to_bytes(body, 8 * 1024 * 1024).await.map_err(|_| s3e(StatusCode::BAD_REQUEST, "MalformedXML", "Body too large."))?;
                delete_objects(&s, &user, &sys, &String::from_utf8_lossy(&b)).await
            }
            Method::PUT => Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "Create baskets in the FileHub console; each basket's storage key is its bucket name.")),
            Method::DELETE => Err(s3e(StatusCode::FORBIDDEN, "AccessDenied", "Baskets can't be deleted through the S3 API.")),
            _ => Err(s3e(StatusCode::METHOD_NOT_ALLOWED, "MethodNotAllowed", "The specified method is not allowed against this resource.")),
        };
    }

    let (sys, _) = basket_of(&s, &user, &bucket).await?;
    let chunked = auth.payload_hash.starts_with("STREAMING-")
        || headers.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).map(|v| v.contains("aws-chunked")).unwrap_or(false);

    match method {
        // Sub-resources first: `PUT ?tagging` must never be taken for a PutObject.
        _ if q.contains_key("retention") || q.contains_key("legal-hold") || q.contains_key("tagging") => {
            let f = find_object(&s, &user, &sys, &key).await?;
            let b = axum::body::to_bytes(body, 64 * 1024).await.map_err(|_| s3e(StatusCode::BAD_REQUEST, "MalformedXML", "Body too large."))?;
            object_sub_resource(&s, &user, &f, &method, &q, &String::from_utf8_lossy(&b)).await
        }
        Method::PUT if q.contains_key("partNumber") && q.contains_key("uploadId") => {
            let up = q["uploadId"].clone();
            let n: i32 = q["partNumber"].parse().map_err(|_| s3e(StatusCode::BAD_REQUEST, "InvalidArgument", "Bad partNumber."))?;
            if !(1..=10000).contains(&n) { return Err(s3e(StatusCode::BAD_REQUEST, "InvalidArgument", "Part number must be 1–10000.")); }
            mpu_owned(&s, &user, &up, &sys, &key).await?;
            let (path, size, md5) = spool(body, &auth.payload_hash, chunked).await?;
            sqlx::query("INSERT INTO s3_parts (upload_id, part_number, path, size, etag) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (upload_id, part_number) DO UPDATE SET path=EXCLUDED.path, size=EXCLUDED.size, etag=EXCLUDED.etag")
                .bind(&up).bind(n).bind(path.to_string_lossy().to_string()).bind(size as i64).bind(&md5).execute(&s.db).await?;
            Ok((StatusCode::OK, [(header::ETAG, format!("\"{md5}\""))]).into_response())
        }
        Method::PUT if headers.contains_key("x-amz-copy-source") => {
            let src = pct_dec(headers["x-amz-copy-source"].to_str().unwrap_or("")).trim_start_matches('/').to_string();
            let (sb, sk) = src.split_once('/').ok_or_else(|| s3e(StatusCode::BAD_REQUEST, "InvalidArgument", "Bad x-amz-copy-source."))?;
            let sk = sk.split('?').next().unwrap_or(sk);
            let (ssys, _) = basket_of(&s, &user, sb).await?;
            let from = find_object(&s, &user, &ssys, sk).await?;
            // Stream the source to the spool area: memory stays O(chunk) for any size.
            let mut src = s.storage.open_stream(&from.object_key, from.encrypted).await?.ok_or(ApiError::NotFound)?;
            let dir = s3_tmp(); tokio::fs::create_dir_all(&dir).await?;
            let p = dir.join(format!("copy-{}", crate::Id::new()));
            let mut out = tokio::fs::File::create(&p).await?;
            while let Some(chunk) = src.next().await { out.write_all(&chunk?).await?; }
            out.flush().await?;
            let mut h2 = headers.clone();
            if headers.get("x-amz-metadata-directive").and_then(|v| v.to_str().ok()) != Some("REPLACE") {
                if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(&from.metadata) {
                    for (k, v) in m { if let (Ok(hn), Some(vs)) = (axum::http::HeaderName::from_bytes(format!("x-amz-meta-{}", k.replace('_', "-")).as_bytes()), v.as_str().map(str::to_string).or_else(|| Some(v.to_string()))) { if let Ok(hv) = HeaderValue::from_str(&urlencoding::encode(&vs)) { h2.insert(hn, hv); } } }
                }
            }
            let (etag, version) = put_object_from(&s, &user, &sys, &key, p, &h2).await?;
            let now = iso(Utc::now());
            let mut r = xml_resp(StatusCode::OK, format!("<CopyObjectResult><LastModified>{now}</LastModified><ETag>\"{etag}\"</ETag></CopyObjectResult>"));
            r.headers_mut().insert("x-amz-version-id", HeaderValue::from(version));
            Ok(r)
        }
        Method::PUT => {
            let (path, _size, _md5) = spool(body, &auth.payload_hash, chunked).await?;
            let (etag, version) = put_object_from(&s, &user, &sys, &key, path, &headers).await?;
            Ok((StatusCode::OK, [(header::ETAG, format!("\"{etag}\"")), (axum::http::HeaderName::from_static("x-amz-version-id"), version.to_string())]).into_response())
        }
        Method::POST if q.contains_key("uploads") => {
            crate::acl::require_folder(&s.db, &user, &sys, None, crate::acl::VIEW).await.map_err(|_| s3e(StatusCode::FORBIDDEN, "AccessDenied", "Access Denied"))?;
            // Refuse a file type the basket won't take before any part is sent.
            let (_, fname) = split_key(&key);
            if !fname.is_empty() { crate::basket::check_policy(&s.db, &sys, None, &fname, 0).await?; }
            let id = crate::Id::new().to_string();
            let mut meta = serde_json::Map::new();
            for (k, v) in &headers { if k.as_str().starts_with("x-amz-meta-") || k == header::CONTENT_TYPE { meta.insert(k.as_str().to_string(), serde_json::Value::String(v.to_str().unwrap_or("").to_string())); } }
            sqlx::query("INSERT INTO s3_multipart (upload_id, system_id, object_key, user_id, headers) VALUES ($1,$2,$3,$4,$5)")
                .bind(&id).bind(&sys).bind(&key).bind(&user.id).bind(serde_json::Value::Object(meta)).execute(&s.db).await?;
            Ok(xml_resp(StatusCode::OK, format!("<InitiateMultipartUploadResult {XMLNS}><Bucket>{}</Bucket><Key>{}</Key><UploadId>{id}</UploadId></InitiateMultipartUploadResult>", xml(&bucket), xml(&key))))
        }
        Method::POST if q.contains_key("uploadId") => {
            let up = q["uploadId"].clone();
            let hdrs = mpu_owned(&s, &user, &up, &sys, &key).await?;
            let b = axum::body::to_bytes(body, 4 * 1024 * 1024).await.map_err(|_| s3e(StatusCode::BAD_REQUEST, "MalformedXML", "Body too large."))?;
            let want: Vec<i32> = tag_values(&String::from_utf8_lossy(&b), "PartNumber").iter().filter_map(|v| v.parse().ok()).collect();
            let parts: Vec<(i32, String, String)> = sqlx::query_as("SELECT part_number, path, etag FROM s3_parts WHERE upload_id = $1 ORDER BY part_number").bind(&up).fetch_all(&s.db).await?;
            let order: Vec<&(i32, String, String)> = if want.is_empty() { parts.iter().collect() } else {
                let mut v = Vec::new();
                for n in &want { v.push(parts.iter().find(|p| p.0 == *n).ok_or_else(|| s3e(StatusCode::BAD_REQUEST, "InvalidPart", format!("Part {n} was not uploaded.")))?); }
                v
            };
            if order.is_empty() { return Err(s3e(StatusCode::BAD_REQUEST, "MalformedXML", "No parts.")); }
            let dir = s3_tmp();
            let joined = dir.join(format!("mpu-{}", crate::Id::new()));
            let mut w = tokio::fs::File::create(&joined).await?;
            let mut md5s = Vec::new();
            for (_, p, e) in &order {
                let mut r = tokio::fs::File::open(p).await?;
                tokio::io::copy(&mut r, &mut w).await?;
                md5s.extend(hex::decode(e).unwrap_or_default());
            }
            w.flush().await?; drop(w);
            let mut h2 = HeaderMap::new();
            if let serde_json::Value::Object(m) = hdrs { for (k, v) in m { if let (Ok(hn), Some(vs)) = (axum::http::HeaderName::from_bytes(k.as_bytes()), v.as_str()) { if let Ok(hv) = HeaderValue::from_str(vs) { h2.insert(hn, hv); } } } }
            let (_, version) = put_object_from(&s, &user, &sys, &key, joined, &h2).await?;
            for (_, p, _) in &parts { let _ = tokio::fs::remove_file(p).await; }
            sqlx::query("DELETE FROM s3_multipart WHERE upload_id = $1").bind(&up).execute(&s.db).await?;
            let etag = format!("{}-{}", hex::encode(Md5::digest(&md5s)), order.len());
            let mut r = xml_resp(StatusCode::OK, format!("<CompleteMultipartUploadResult {XMLNS}><Bucket>{}</Bucket><Key>{}</Key><ETag>\"{etag}\"</ETag></CompleteMultipartUploadResult>", xml(&bucket), xml(&key)));
            r.headers_mut().insert("x-amz-version-id", HeaderValue::from(version));
            Ok(r)
        }
        Method::DELETE if q.contains_key("uploadId") => {
            let up = q["uploadId"].clone();
            mpu_owned(&s, &user, &up, &sys, &key).await?;
            let parts: Vec<(String,)> = sqlx::query_as("SELECT path FROM s3_parts WHERE upload_id = $1").bind(&up).fetch_all(&s.db).await?;
            for (p,) in parts { let _ = tokio::fs::remove_file(p).await; }
            sqlx::query("DELETE FROM s3_multipart WHERE upload_id = $1").bind(&up).execute(&s.db).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::GET if q.contains_key("uploadId") => list_parts(&s, &user, &sys, &bucket, &key, &q).await,
        Method::GET | Method::HEAD => {
            let f = find_object(&s, &user, &sys, &key).await?;
            let want = q.get("versionId").map(String::as_str);
            let (obj_key, encrypted, size, etag, modified) = version_of(&s, &f, want).await?;
            let version = want.and_then(|v| v.parse::<i64>().ok()).unwrap_or(f.version);
            let mut resp = if method == Method::HEAD {
                let mut r = StatusCode::OK.into_response();
                r.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(size as u64));
                r.headers_mut().insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
                if let Some(e) = &etag { if let Ok(v) = HeaderValue::from_str(&format!("\"{e}\"")) { r.headers_mut().insert(header::ETAG, v); } }
                r
            } else {
                crate::serve::stream_object(&s, crate::serve::ServeOpts {
                    key: &obj_key, encrypted, name: &f.name, etag: etag.as_deref(),
                    disposition: "attachment", cache_control: "private, no-store",
                }, &headers).await?
            };
            let h = resp.headers_mut();
            if let Ok(v) = HeaderValue::from_str(&modified.format("%a, %d %b %Y %H:%M:%S GMT").to_string()) { h.insert(header::LAST_MODIFIED, v); }
            let ct = mime_guess::from_path(&f.name).first().map(|m| m.to_string()).unwrap_or_else(|| "application/octet-stream".into());
            if let Ok(v) = HeaderValue::from_str(&ct) { h.insert(header::CONTENT_TYPE, v); }
            if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(&f.metadata) {
                for (k, v) in m {
                    let vs = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
                    if let (Ok(hn), Ok(hv)) = (axum::http::HeaderName::from_bytes(format!("x-amz-meta-{}", k.replace('_', "-")).as_bytes()), HeaderValue::from_str(&urlencoding::encode(&vs))) { h.insert(hn, hv); }
                }
            }
            if let Some(c) = &f.category_code { if let Ok(hv) = HeaderValue::from_str(c) { h.insert("x-amz-meta-category", hv); } }
            h.insert("x-amz-version-id", HeaderValue::from(version));
            Ok(resp)
        }
        Method::DELETE if q.contains_key("versionId") => Err(s3e(StatusCode::FORBIDDEN, "AccessDenied",
            "FileHub keeps every version. Delete the object to send it to Trash; past versions stay with it.")),
        Method::DELETE => {
            match find_object(&s, &user, &sys, &key).await {
                Ok(f) => crate::handlers::trash_file(&s, &user, &f, " via S3").await?,
                Err(e) if e.status == StatusCode::NOT_FOUND => {} // S3 deletes are idempotent
                Err(e) => return Err(e),
            }
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        _ => Err(s3e(StatusCode::METHOD_NOT_ALLOWED, "MethodNotAllowed", "The specified method is not allowed against this resource.")),
    }
}

async fn mpu_owned(s: &AppState, user: &User, up: &str, sys: &str, key: &str) -> S3Result<serde_json::Value> {
    let row: Option<(String, String, String, serde_json::Value)> = sqlx::query_as("SELECT system_id, object_key, user_id, headers FROM s3_multipart WHERE upload_id = $1")
        .bind(up).fetch_optional(&s.db).await?;
    match row {
        Some((a, b, c, h)) if a == sys && b == key && c == user.id => Ok(h),
        _ => Err(s3e(StatusCode::NOT_FOUND, "NoSuchUpload", "The specified multipart upload does not exist.")),
    }
}

// ── Object lock + tagging sub-resources ──

/// GET/PUT `?retention`, `?legal-hold`, and GET/PUT/DELETE `?tagging` on an object.
async fn object_sub_resource(s: &AppState, user: &User, f: &File, method: &Method, q: &BTreeMap<String, String>, body: &str) -> S3Result<Response> {
    let lock = crate::objectlock::get(&s.db, f.id).await?;
    if q.contains_key("retention") {
        if *method == Method::GET {
            let (Some(mode), Some(until)) = (&lock.mode, lock.retain_until) else {
                return Err(s3e(StatusCode::NOT_FOUND, "NoSuchObjectLockConfiguration", "The specified object does not have a retention configuration."));
            };
            return Ok(xml_resp(StatusCode::OK, format!("<Retention {XMLNS}><Mode>{mode}</Mode><RetainUntilDate>{}</RetainUntilDate></Retention>", iso(until))));
        }
        let mode = tag_values(body, "Mode").into_iter().next();
        let until = tag_values(body, "RetainUntilDate").into_iter().next()
            .map(|t| DateTime::parse_from_rfc3339(&t).map(|d| d.with_timezone(&Utc)))
            .transpose().map_err(|_| s3e(StatusCode::BAD_REQUEST, "InvalidArgument", "RetainUntilDate must be an ISO 8601 date."))?;
        crate::objectlock::set(&s.db, f.id, user, &crate::objectlock::Lock { mode, retain_until: until, legal_hold: lock.legal_hold }).await?;
        return Ok(StatusCode::OK.into_response());
    }
    if q.contains_key("legal-hold") {
        if *method == Method::GET {
            let status = if lock.legal_hold { "ON" } else { "OFF" };
            return Ok(xml_resp(StatusCode::OK, format!("<LegalHold {XMLNS}><Status>{status}</Status></LegalHold>")));
        }
        let on = tag_values(body, "Status").first().map(|v| v == "ON").unwrap_or(false);
        crate::objectlock::set(&s.db, f.id, user, &crate::objectlock::Lock { legal_hold: on, ..lock }).await?;
        return Ok(StatusCode::OK.into_response());
    }
    // Tagging: S3 tags live in FileHub's tag list as "key:value" (searchable in the
    // app). FileHub's own automatic tags (basket, agency, year, uploader) are not
    // S3 tags: hidden from GET, kept on PUT/DELETE.
    const AUTO: [&str; 4] = ["system", "org", "fiscal-year", "uploaded-by"];
    let is_s3_tag = |t: &String| t.split_once(':').map(|(k, _)| !AUTO.contains(&k)).unwrap_or(false);
    let mut tags: Vec<String> = serde_json::from_str(&f.tags).unwrap_or_default();
    match *method {
        Method::GET => {
            let set: String = tags.iter().filter(|t| is_s3_tag(t)).filter_map(|t| t.split_once(':'))
                .map(|(k, v)| format!("<Tag><Key>{}</Key><Value>{}</Value></Tag>", xml(k), xml(v))).collect();
            Ok(xml_resp(StatusCode::OK, format!("<Tagging {XMLNS}><TagSet>{set}</TagSet></Tagging>")))
        }
        Method::PUT | Method::DELETE => {
            crate::acl::require_file(&s.db, user, f.id, crate::acl::EDIT).await?;
            tags.retain(|t| !is_s3_tag(t));
            if *method == Method::PUT {
                let (keys, values) = (tag_values(body, "Key"), tag_values(body, "Value"));
                if keys.len() > 10 { return Err(s3e(StatusCode::BAD_REQUEST, "BadRequest", "Object tags cannot be greater than 10.")); }
                tags.extend(keys.into_iter().zip(values).map(|(k, v)| format!("{k}:{v}")));
            }
            sqlx::query("UPDATE files SET tags = $1 WHERE id = $2").bind(serde_json::to_string(&tags).unwrap_or_default()).bind(f.id).execute(&s.db).await?;
            Ok(if *method == Method::PUT { StatusCode::OK } else { StatusCode::NO_CONTENT }.into_response())
        }
        _ => Err(s3e(StatusCode::METHOD_NOT_ALLOWED, "MethodNotAllowed", "The specified method is not allowed against this resource.")),
    }
}

/// GET `?object-lock` on a bucket: the basket's default lock (set in the console).
async fn bucket_lock_config(s: &AppState, sys: &str) -> S3Result<Response> {
    let p = crate::basket::effective_policy(&s.db, sys, None).await?;
    let rule = match (p.lock_days, p.lock_mode) {
        (Some(d), Some(m)) if d > 0 => format!("<Rule><DefaultRetention><Mode>{m}</Mode><Days>{d}</Days></DefaultRetention></Rule>"),
        _ => String::new(),
    };
    Ok(xml_resp(StatusCode::OK, format!("<ObjectLockConfiguration {XMLNS}><ObjectLockEnabled>Enabled</ObjectLockEnabled>{rule}</ObjectLockConfiguration>")))
}

/// ListObjectVersions — each visible file's current version and its history.
/// Version ids are FileHub's version numbers ("1", "2", …).
async fn list_versions(s: &AppState, user: &User, bucket: &str, q: &BTreeMap<String, String>) -> S3Result<Response> {
    let (sys, _) = basket_of(s, user, bucket).await?;
    let prefix = q.get("prefix").cloned().unwrap_or_default();
    let after = q.get("key-marker").cloned().unwrap_or_default();
    let max: usize = q.get("max-keys").and_then(|v| v.parse().ok()).unwrap_or(1000).clamp(1, 1000);
    let mut rows = visible_objects(s, user, &sys, &prefix, &after).await?;
    let truncated = rows.len() > max;
    rows.truncate(max);
    let ids: Vec<Id> = rows.iter().map(|r| r.id).collect();
    let older: Vec<(Id, i64, i64, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT file_id, version, size_bytes, etag, created_at FROM file_versions WHERE file_id = ANY($1) ORDER BY file_id, version DESC",
    ).bind(&ids).fetch_all(&s.db).await?;
    let entry = |key: &str, v: i64, latest: bool, at: DateTime<Utc>, etag: &Option<String>, size: i64| format!(
        "<Version><Key>{}</Key><VersionId>{v}</VersionId><IsLatest>{latest}</IsLatest><LastModified>{}</LastModified><ETag>\"{}\"</ETag><Size>{size}</Size><StorageClass>STANDARD</StorageClass></Version>",
        xml(key), iso(at), etag.as_deref().unwrap_or(""));
    let mut x = format!("<ListVersionsResult {XMLNS}><Name>{}</Name><Prefix>{}</Prefix><KeyMarker>{}</KeyMarker><MaxKeys>{max}</MaxKeys><IsTruncated>{truncated}</IsTruncated>",
        xml(bucket), xml(&prefix), xml(&after));
    if truncated { if let Some(last) = rows.last() { x.push_str(&format!("<NextKeyMarker>{}</NextKeyMarker>", xml(&last.key))); } }
    for r in &rows {
        x.push_str(&entry(&r.key, r.version, true, r.modified_at, &r.etag, r.size_bytes));
        for (_, v, size, etag, at) in older.iter().filter(|o| o.0 == r.id) { x.push_str(&entry(&r.key, *v, false, *at, etag, *size)); }
    }
    x.push_str("</ListVersionsResult>");
    Ok(xml_resp(StatusCode::OK, x))
}

/// A stored version of `f` (`None` = the current one), as (object key, encrypted, size, etag, time).
async fn version_of(s: &AppState, f: &File, version: Option<&str>) -> S3Result<(String, bool, i64, Option<String>, DateTime<Utc>)> {
    let current = (f.object_key.clone(), f.encrypted, f.size_bytes, f.etag.clone(), f.modified_at);
    let Some(v) = version.filter(|v| !v.is_empty() && *v != "null") else { return Ok(current) };
    let n: i64 = v.parse().map_err(|_| s3e(StatusCode::BAD_REQUEST, "InvalidArgument", "Invalid version id."))?;
    if n == f.version { return Ok(current); }
    let row: Option<(String, Option<bool>, i64, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT object_key, encrypted, size_bytes, etag, created_at FROM file_versions WHERE file_id = $1 AND version = $2",
    ).bind(f.id).bind(n).fetch_optional(&s.db).await?;
    let (key, enc, size, etag, at) = row.ok_or_else(|| s3e(StatusCode::NOT_FOUND, "NoSuchVersion", "The specified version does not exist."))?;
    Ok((key, enc.unwrap_or(f.encrypted), size, etag, at))
}

/// ListMultipartUploads — the caller's own unfinished uploads in a bucket
/// (clients use it to find uploads they can resume or clean up).
async fn list_uploads(s: &AppState, user: &User, sys: &str, bucket: &str, q: &BTreeMap<String, String>) -> S3Result<Response> {
    let prefix = q.get("prefix").cloned().unwrap_or_default();
    let rows: Vec<(String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT upload_id, object_key, created_at FROM s3_multipart
          WHERE system_id = $1 AND user_id = $2 AND starts_with(object_key, $3)
          ORDER BY object_key, created_at LIMIT 1000",
    ).bind(sys).bind(&user.id).bind(&prefix).fetch_all(&s.db).await?;
    let items: String = rows.iter().map(|(id, key, at)| format!(
        "<Upload><Key>{}</Key><UploadId>{id}</UploadId><Initiated>{}</Initiated><StorageClass>STANDARD</StorageClass></Upload>",
        xml(key), iso(*at))).collect();
    Ok(xml_resp(StatusCode::OK, format!(
        "<ListMultipartUploadsResult {XMLNS}><Bucket>{}</Bucket><Prefix>{}</Prefix><MaxUploads>1000</MaxUploads><IsTruncated>false</IsTruncated>{items}</ListMultipartUploadsResult>",
        xml(bucket), xml(&prefix))))
}

/// ListParts — which parts of an upload already arrived (to resume it).
async fn list_parts(s: &AppState, user: &User, sys: &str, bucket: &str, key: &str, q: &BTreeMap<String, String>) -> S3Result<Response> {
    let up = q["uploadId"].clone();
    mpu_owned(s, user, &up, sys, key).await?;
    let after: i32 = q.get("part-number-marker").and_then(|v| v.parse().ok()).unwrap_or(0);
    let max: i64 = q.get("max-parts").and_then(|v| v.parse().ok()).filter(|v: &i64| (1..=1000).contains(v)).unwrap_or(1000);
    let (started,): (DateTime<Utc>,) = sqlx::query_as("SELECT created_at FROM s3_multipart WHERE upload_id = $1").bind(&up).fetch_one(&s.db).await?;
    let rows: Vec<(i32, i64, String)> = sqlx::query_as(
        "SELECT part_number, size, etag FROM s3_parts WHERE upload_id = $1 AND part_number > $2 ORDER BY part_number LIMIT $3",
    ).bind(&up).bind(after).bind(max + 1).fetch_all(&s.db).await?;
    let truncated = rows.len() as i64 > max;
    let shown = &rows[..rows.len().min(max as usize)];
    let next = shown.last().map(|r| r.0).unwrap_or(after);
    let parts: String = shown.iter().map(|(n, size, etag)| format!(
        "<Part><PartNumber>{n}</PartNumber><LastModified>{}</LastModified><ETag>\"{etag}\"</ETag><Size>{size}</Size></Part>", iso(started))).collect();
    Ok(xml_resp(StatusCode::OK, format!(
        "<ListPartsResult {XMLNS}><Bucket>{}</Bucket><Key>{}</Key><UploadId>{up}</UploadId><PartNumberMarker>{after}</PartNumberMarker><NextPartNumberMarker>{next}</NextPartNumberMarker><MaxParts>{max}</MaxParts><IsTruncated>{truncated}</IsTruncated><StorageClass>STANDARD</StorageClass>{parts}</ListPartsResult>",
        xml(bucket), xml(key))))
}

fn tag_values(x: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = x;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(&close) else { break };
        out.push(after[..j].replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'"));
        rest = &after[j + close.len()..];
    }
    out
}

async fn delete_objects(s: &Arc<AppState>, user: &User, sys: &str, body: &str) -> S3Result<Response> {
    let quiet = body.contains("<Quiet>true</Quiet>");
    let mut out = format!("<DeleteResult {XMLNS}>");
    for key in tag_values(body, "Key") {
        let r: S3Result<()> = async {
            if let Ok(f) = find_object(s, user, sys, &key).await {
                crate::handlers::trash_file(s, user, &f, " via S3").await?;
            }
            Ok(())
        }.await;
        match r {
            Ok(()) => if !quiet { out.push_str(&format!("<Deleted><Key>{}</Key></Deleted>", xml(&key))) },
            Err(e) => out.push_str(&format!("<Error><Key>{}</Key><Code>{}</Code><Message>{}</Message></Error>", xml(&key), e.code, xml(&e.msg))),
        }
    }
    out.push_str("</DeleteResult>");
    Ok(xml_resp(StatusCode::OK, out))
}

async fn list_buckets(s: &AppState, user: &User) -> S3Result<Response> {
    let rows: Vec<(String, DateTime<Utc>, String)> = match crate::auth::effective_system_ids(&s.db, user).await? {
        None => sqlx::query_as("SELECT bucket, created_at, id FROM systems WHERE deleted_at IS NULL ORDER BY bucket").fetch_all(&s.db).await?,
        Some(ids) => sqlx::query_as("SELECT bucket, created_at, id FROM systems WHERE deleted_at IS NULL AND id = ANY($1) ORDER BY bucket").bind(&ids).fetch_all(&s.db).await?,
    };
    let mut x = format!("<ListAllMyBucketsResult {XMLNS}><Owner><ID>{}</ID><DisplayName>{}</DisplayName></Owner><Buckets>", xml(&user.id), xml(&user.display_name));
    for (b, t, _) in rows { x.push_str(&format!("<Bucket><Name>{}</Name><CreationDate>{}</CreationDate></Bucket>", xml(&b), iso(t))); }
    x.push_str("</Buckets></ListAllMyBucketsResult>");
    Ok(xml_resp(StatusCode::OK, x))
}

#[derive(FromRow)]
struct ObjRow { key: String, id: Id, version: i64, size_bytes: i64, etag: Option<String>, modified_at: DateTime<Utc> }

/// Every file of a basket the caller may see, keyed by its folder path, from
/// `after` on, under `prefix` (one row per key: the newest file wins).
async fn visible_objects(s: &AppState, user: &User, sys: &str, prefix: &str, after: &str) -> S3Result<Vec<ObjRow>> {
    let like = format!("{}%", prefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    // Admins see everything; others get the set-based visibility filter.
    let (access, vis) = if user.role == "admin" { (String::new(), None) } else {
        (" AND ((fi.folder_id IS NULL AND fi.system_id = ANY($5)) OR fi.folder_id = ANY($6) OR fi.id::text = ANY($7) OR fi.created_by = $8)".to_string(),
         Some(crate::acl::visibility(&s.db, user, &[sys.to_string()]).await?))
    };
    let sql = format!(r#"WITH RECURSIVE fp(id, path) AS (
              SELECT id, name::text FROM folders WHERE system_id = $1 AND parent_id IS NULL AND deleted_at IS NULL
              UNION ALL
              SELECT f.id, fp.path || '/' || f.name FROM folders f JOIN fp ON f.parent_id = fp.id WHERE f.deleted_at IS NULL)
           SELECT * FROM (
             SELECT DISTINCT ON (key) key, id, version, size_bytes, etag, modified_at FROM (
               SELECT COALESCE(fp.path || '/', '') || fi.name AS key, fi.id, fi.version, fi.size_bytes, fi.etag, fi.modified_at
                 FROM files fi LEFT JOIN fp ON fp.id = fi.folder_id
                WHERE fi.system_id = $1 AND fi.deleted_at IS NULL AND (fi.folder_id IS NULL OR fp.id IS NOT NULL){access}
             ) x ORDER BY key, modified_at DESC
           ) y
           WHERE key LIKE $2 AND key > $3
           ORDER BY key COLLATE "C"
           LIMIT $4"#);
    let mut q = sqlx::query_as::<_, ObjRow>(&sql).bind(sys).bind(&like).bind(after).bind(20000_i64);
    if let Some(v) = vis { q = q.bind(v.baskets).bind(v.folders).bind(v.files).bind(&user.id); }
    Ok(q.fetch_all(&s.db).await?)
}

async fn list_objects(s: &AppState, user: &User, bucket: &str, q: &BTreeMap<String, String>) -> S3Result<Response> {
    let (sys, _) = basket_of(s, user, bucket).await?;
    let v2 = q.get("list-type").map(|v| v == "2").unwrap_or(false);
    let prefix = q.get("prefix").cloned().unwrap_or_default();
    let delim = q.get("delimiter").cloned().filter(|d| !d.is_empty());
    let max: usize = q.get("max-keys").and_then(|v| v.parse().ok()).unwrap_or(1000).clamp(1, 1000);
    let after = if v2 {
        q.get("continuation-token").and_then(|t| B64.decode(t).ok()).and_then(|b| String::from_utf8(b).ok())
            .or_else(|| q.get("start-after").cloned()).unwrap_or_default()
    } else { q.get("marker").cloned().unwrap_or_default() };
    let rows = visible_objects(s, user, &sys, &prefix, &after).await?;

    let mut contents = Vec::new();
    let mut prefixes: Vec<String> = Vec::new();
    let mut last = String::new();
    let mut truncated = false;
    for r in rows {
        if contents.len() + prefixes.len() >= max { truncated = true; break; }
        if let Some(d) = &delim {
            if let Some(i) = r.key[prefix.len()..].find(d.as_str()) {
                let cp = r.key[..prefix.len() + i + d.len()].to_string();
                if prefixes.last() != Some(&cp) && !prefixes.contains(&cp) { prefixes.push(cp.clone()); }
                last = r.key.clone();
                continue;
            }
        }
        last = r.key.clone();
        contents.push(r);
    }
    let mut x = format!("<ListBucketResult {XMLNS}><Name>{}</Name><Prefix>{}</Prefix><MaxKeys>{max}</MaxKeys><IsTruncated>{truncated}</IsTruncated>", xml(bucket), xml(&prefix));
    if let Some(d) = &delim { x.push_str(&format!("<Delimiter>{}</Delimiter>", xml(d))); }
    if v2 {
        x.push_str(&format!("<KeyCount>{}</KeyCount>", contents.len() + prefixes.len()));
        if truncated { x.push_str(&format!("<NextContinuationToken>{}</NextContinuationToken>", B64.encode(last.as_bytes()))); }
        if let Some(t) = q.get("continuation-token") { x.push_str(&format!("<ContinuationToken>{}</ContinuationToken>", xml(t))); }
    } else if truncated {
        x.push_str(&format!("<NextMarker>{}</NextMarker>", xml(&last)));
    }
    for c in contents {
        x.push_str(&format!("<Contents><Key>{}</Key><LastModified>{}</LastModified><ETag>\"{}\"</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
            xml(&c.key), iso(c.modified_at), c.etag.unwrap_or_default(), c.size_bytes));
    }
    for p in prefixes { x.push_str(&format!("<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>", xml(&p))); }
    x.push_str("</ListBucketResult>");
    Ok(xml_resp(StatusCode::OK, x))
}

/// Cleans multipart uploads abandoned for over a day (called by rotation).
pub async fn reap_multipart(db: &sqlx::PgPool) {
    let old: Vec<(String,)> = sqlx::query_as("SELECT upload_id FROM s3_multipart WHERE created_at < now() - interval '1 day'").fetch_all(db).await.unwrap_or_default();
    for (up,) in old {
        let parts: Vec<(String,)> = sqlx::query_as("SELECT path FROM s3_parts WHERE upload_id = $1").bind(&up).fetch_all(db).await.unwrap_or_default();
        for (p,) in parts { let _ = tokio::fs::remove_file(p).await; }
        let _ = sqlx::query("DELETE FROM s3_multipart WHERE upload_id = $1").bind(&up).execute(db).await;
    }
}

// Silence unused import warnings for items only used in some cfgs.
#[allow(dead_code)]
fn _unused(_: AuthUser) {}

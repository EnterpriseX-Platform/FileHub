//! Electronic signatures (TOR Annex A: ระบบลายมือชื่ออิเล็กทรอนิกส์).
//!
//! Implements ETA พ.ร.บ. 2544 §9-style electronic signatures: a signer is
//! authenticated (session), the signing action is recorded with an integrity
//! hash of the document (SHA-256) at signing time, and the whole chain is
//! verifiable. Supports a reusable signature library, single- and multi-signer
//! requests in **sequential** or **parallel** order, expiry, decline, and
//! verification. Sign-turn notifications reuse the in-app notifications feed.
//!
//! Visible placement (page + x/y/w/h) is stored so the viewer overlays the
//! signature mark on the preview. True PDF byte-embedding (PAdES) and PKI
//! digital certificates (§26 secure signatures) are a pluggable extension on
//! top of this — the data model already carries what they need.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::auth::{ensure_system_access, require_role, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

/// SHA-256 (hex) of the file's current plaintext bytes, or None if unreadable.
async fn doc_hash(state: &AppState, file_id: Uuid) -> Option<String> {
    let (object_key, encrypted): (String, bool) =
        sqlx::query_as("SELECT object_key, encrypted FROM files WHERE id = $1 AND deleted_at IS NULL")
            .bind(file_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()?;
    let (bytes, _) = state.storage.get(&object_key, encrypted).await.ok().flatten()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(hex(&h.finalize()))
}

/// Confirm the file exists and the caller may see it; 404 otherwise. Returns system_id.
async fn access_or_404(state: &AppState, user: &AuthUser, id: Uuid) -> ApiResult<String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    if ensure_system_access(&state.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    Ok(system_id)
}

fn client_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
}

async fn notify(state: &AppState, user_id: &str, title: &str, body: &str, file_id: Uuid) {
    let _ = sqlx::query(
        "INSERT INTO notifications (id, user_id, kind, title, body, link) VALUES ($1,$2,'signature',$3,$4,$5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(title)
    .bind(body)
    .bind(format!("/f/{file_id}"))
    .execute(&state.db)
    .await;

    // Mirror to email when configured — sign-turn/complete alerts (ANNEX-38).
    // Spawned so SMTP latency never slows the request; no-op when mail is off.
    if crate::mailer::enabled() {
        if let Ok(Some((email,))) =
            sqlx::query_as::<_, (String,)>("SELECT email FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&state.db)
                .await
        {
            let subject = title.to_string();
            let text = format!("{body}\n\nOpen: {}/f/{file_id}", crate::mailer::base_url());
            tokio::spawn(async move {
                if let Err(e) = crate::mailer::send(&email, &subject, &text).await {
                    tracing::warn!("signature email failed: {e}");
                }
            });
        }
    }
}

// ---- Signature library ------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct Signature {
    pub id: Uuid,
    pub label: String,
    pub kind: String,
    pub image: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct NewSignature {
    pub label: Option<String>,
    pub kind: Option<String>,
    pub image: String,
}

/// GET /fh/api/signatures — the caller's signature library.
pub async fn list_signatures(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<Signature>>> {
    let rows = sqlx::query_as::<_, Signature>(
        "SELECT id, label, kind, image, created_at FROM signatures WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(&user.0.id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(rows))
}

/// POST /fh/api/signatures — save a signature mark.
pub async fn create_signature(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Json(b): Json<NewSignature>,
) -> ApiResult<Json<Signature>> {
    if b.image.trim().is_empty() {
        return Err(ApiError::BadRequest("image is required".into()));
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO signatures (id, user_id, label, kind, image) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(&user.0.id)
    .bind(b.label.unwrap_or_else(|| "Signature".into()))
    .bind(b.kind.unwrap_or_else(|| "drawn".into()))
    .bind(&b.image)
    .execute(&s.db)
    .await?;
    let row = sqlx::query_as::<_, Signature>(
        "SELECT id, label, kind, image, created_at FROM signatures WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&s.db)
    .await?;
    Ok(Json(row))
}

/// DELETE /fh/api/signatures/:id
pub async fn delete_signature(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusOk> {
    sqlx::query("DELETE FROM signatures WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(&user.0.id)
        .execute(&s.db)
        .await?;
    Ok(StatusOk)
}

pub struct StatusOk;
impl axum::response::IntoResponse for StatusOk {
    fn into_response(self) -> axum::response::Response {
        axum::http::StatusCode::NO_CONTENT.into_response()
    }
}

// ---- Sign requests ----------------------------------------------------------

#[derive(Deserialize)]
pub struct SignerInput {
    pub user_id: String,
    pub seq: Option<i32>,
    pub page: Option<i32>,
    pub pos_x: Option<f32>,
    pub pos_y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

#[derive(Deserialize)]
pub struct NewSignRequest {
    pub order_mode: Option<String>,
    pub message: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub signers: Vec<SignerInput>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SignerView {
    pub id: Uuid,
    pub user_id: String,
    pub user_name: Option<String>,
    pub seq: i32,
    pub status: String,
    pub page: Option<i32>,
    pub pos_x: Option<f32>,
    pub pos_y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub signature_id: Option<Uuid>,
    pub signed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct RequestRow {
    pub id: Uuid,
    pub file_id: Uuid,
    pub created_by: String,
    pub order_mode: String,
    pub status: String,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct RequestView {
    #[serde(flatten)]
    pub request: RequestRow,
    pub signers: Vec<SignerView>,
}

async fn load_signers(s: &AppState, request_id: Uuid) -> ApiResult<Vec<SignerView>> {
    Ok(sqlx::query_as::<_, SignerView>(
        "SELECT ss.id, ss.user_id, u.display_name AS user_name, ss.seq, ss.status, ss.page, \
                ss.pos_x, ss.pos_y, ss.width, ss.height, ss.signature_id, ss.signed_at \
         FROM signature_signers ss LEFT JOIN users u ON u.id = ss.user_id \
         WHERE ss.request_id = $1 ORDER BY ss.seq, ss.id",
    )
    .bind(request_id)
    .fetch_all(&s.db)
    .await?)
}

async fn load_request(s: &AppState, id: Uuid) -> ApiResult<RequestRow> {
    sqlx::query_as::<_, RequestRow>(
        "SELECT id, file_id, created_by, order_mode, status, message, created_at, expires_at, completed_at \
         FROM signature_requests WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// POST /fh/api/files/:id/sign-requests — start a signing request (editor+).
pub async fn create_request(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
    Json(b): Json<NewSignRequest>,
) -> ApiResult<Json<RequestView>> {
    require_role(&user.0, &["admin", "editor"])?;
    access_or_404(&s, &user, file_id).await?;
    if b.signers.is_empty() {
        return Err(ApiError::BadRequest("at least one signer is required".into()));
    }
    let order_mode = match b.order_mode.as_deref() {
        Some("parallel") => "parallel",
        _ => "sequential",
    };
    let hash = doc_hash(&s, file_id).await;

    let req_id = Uuid::now_v7();
    let mut tx = s.db.begin().await?;
    sqlx::query(
        "INSERT INTO signature_requests (id, file_id, created_by, order_mode, status, message, doc_hash, expires_at) \
         VALUES ($1,$2,$3,$4,'pending',$5,$6,$7)",
    )
    .bind(req_id)
    .bind(file_id)
    .bind(&user.0.id)
    .bind(order_mode)
    .bind(&b.message)
    .bind(&hash)
    .bind(b.expires_at)
    .execute(&mut *tx)
    .await?;

    for (i, sg) in b.signers.iter().enumerate() {
        sqlx::query(
            "INSERT INTO signature_signers (id, request_id, user_id, seq, page, pos_x, pos_y, width, height) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(Uuid::now_v7())
        .bind(req_id)
        .bind(&sg.user_id)
        .bind(sg.seq.unwrap_or(i as i32))
        .bind(sg.page)
        .bind(sg.pos_x)
        .bind(sg.pos_y)
        .bind(sg.width)
        .bind(sg.height)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    // Notify: parallel → everyone now; sequential → only the first in order.
    if order_mode == "parallel" {
        for sg in &b.signers {
            notify(&s, &sg.user_id, "Signature requested", "A document needs your signature.", file_id).await;
        }
    } else if let Some(first) = b.signers.iter().min_by_key(|s| s.seq.unwrap_or(0)) {
        notify(&s, &first.user_id, "Signature requested", "A document needs your signature.", file_id).await;
    }

    let signers = load_signers(&s, req_id).await?;
    let request = load_request(&s, req_id).await?;
    Ok(Json(RequestView { request, signers }))
}

/// GET /fh/api/files/:id/sign-requests — all sign requests on a file.
pub async fn list_file_requests(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Uuid>,
) -> ApiResult<Json<Vec<RequestView>>> {
    access_or_404(&s, &user, file_id).await?;
    let reqs = sqlx::query_as::<_, RequestRow>(
        "SELECT id, file_id, created_by, order_mode, status, message, created_at, expires_at, completed_at \
         FROM signature_requests WHERE file_id = $1 ORDER BY created_at DESC",
    )
    .bind(file_id)
    .fetch_all(&s.db)
    .await?;
    let mut out = Vec::with_capacity(reqs.len());
    for request in reqs {
        let signers = load_signers(&s, request.id).await?;
        out.push(RequestView { request, signers });
    }
    Ok(Json(out))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct QueueItem {
    pub request_id: Uuid,
    pub signer_id: Uuid,
    pub file_id: Uuid,
    pub file_name: String,
    pub message: Option<String>,
    pub order_mode: String,
    pub seq: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub my_turn: bool,
}

/// GET /fh/api/sign-requests/mine — documents awaiting my signature.
pub async fn my_queue(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Vec<QueueItem>>> {
    // A signer's turn is now if parallel, or if no earlier-seq signer is still pending.
    let rows = sqlx::query_as::<_, QueueItem>(
        "SELECT r.id AS request_id, ss.id AS signer_id, r.file_id, f.name AS file_name, \
                r.message, r.order_mode, ss.seq, r.created_at, r.expires_at, \
                (r.order_mode = 'parallel' OR NOT EXISTS ( \
                    SELECT 1 FROM signature_signers e \
                    WHERE e.request_id = r.id AND e.status = 'pending' AND e.seq < ss.seq)) AS my_turn \
         FROM signature_signers ss \
         JOIN signature_requests r ON r.id = ss.request_id \
         JOIN files f ON f.id = r.file_id \
         WHERE ss.user_id = $1 AND ss.status = 'pending' AND r.status = 'pending' \
           AND f.deleted_at IS NULL \
         ORDER BY r.created_at DESC",
    )
    .bind(&user.0.id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct SignInput {
    pub signature_id: Option<Uuid>,
}

/// POST /fh/api/sign-requests/:id/sign — sign (must be an assigned signer, and
/// your turn if the request is sequential).
pub async fn sign(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    headers: HeaderMap,
    Path(request_id): Path<Uuid>,
    Json(b): Json<SignInput>,
) -> ApiResult<Json<RequestView>> {
    let req = load_request(&s, request_id).await?;
    if req.status != "pending" {
        return Err(ApiError::Conflict(format!("request is {}", req.status)));
    }
    if let Some(exp) = req.expires_at {
        if exp < Utc::now() {
            return Err(ApiError::Conflict("this signing request has expired".into()));
        }
    }
    // Find my pending signer row.
    let mine: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, seq FROM signature_signers WHERE request_id = $1 AND user_id = $2 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(&user.0.id)
    .fetch_optional(&s.db)
    .await?;
    let (signer_id, seq) = mine.ok_or(ApiError::NotFound)?;

    // Sequential: everyone before me must be done first.
    if req.order_mode == "sequential" {
        let earlier: Option<(i64,)> = sqlx::query_as(
            "SELECT count(*) FROM signature_signers WHERE request_id = $1 AND status = 'pending' AND seq < $2",
        )
        .bind(request_id)
        .bind(seq)
        .fetch_optional(&s.db)
        .await?;
        if earlier.map(|c| c.0).unwrap_or(0) > 0 {
            return Err(ApiError::Conflict("it isn't your turn to sign yet".into()));
        }
    }

    // Integrity: hash the document as it is at the moment of signing.
    let signed_hash = doc_hash(&s, req.file_id).await;
    let ip = client_ip(&headers);
    sqlx::query(
        "UPDATE signature_signers SET status = 'signed', signature_id = $2, signed_hash = $3, \
                signed_at = now(), ip = $4 WHERE id = $1",
    )
    .bind(signer_id)
    .bind(b.signature_id)
    .bind(&signed_hash)
    .bind(&ip)
    .execute(&s.db)
    .await?;

    // Everyone signed? complete. Else, if sequential, ping the next signer.
    let remaining: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM signature_signers WHERE request_id = $1 AND status = 'pending'",
    )
    .bind(request_id)
    .fetch_one(&s.db)
    .await?;
    if remaining.0 == 0 {
        sqlx::query("UPDATE signature_requests SET status = 'completed', completed_at = now() WHERE id = $1")
            .bind(request_id)
            .execute(&s.db)
            .await?;
        notify(&s, &req.created_by, "Signing complete", "All signers have signed the document.", req.file_id).await;
    } else if req.order_mode == "sequential" {
        let next: Option<(String,)> = sqlx::query_as(
            "SELECT user_id FROM signature_signers WHERE request_id = $1 AND status = 'pending' ORDER BY seq LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&s.db)
        .await?;
        if let Some((uid,)) = next {
            notify(&s, &uid, "Signature requested", "It's your turn to sign a document.", req.file_id).await;
        }
    }

    let request = load_request(&s, request_id).await?;
    let signers = load_signers(&s, request_id).await?;
    Ok(Json(RequestView { request, signers }))
}

/// POST /fh/api/sign-requests/:id/decline
pub async fn decline(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(request_id): Path<Uuid>,
) -> ApiResult<Json<RequestView>> {
    let req = load_request(&s, request_id).await?;
    let updated = sqlx::query(
        "UPDATE signature_signers SET status = 'declined', signed_at = now() \
         WHERE request_id = $1 AND user_id = $2 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(&user.0.id)
    .execute(&s.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query("UPDATE signature_requests SET status = 'declined' WHERE id = $1 AND status = 'pending'")
        .bind(request_id)
        .execute(&s.db)
        .await?;
    notify(&s, &req.created_by, "Signing declined", "A signer declined to sign the document.", req.file_id).await;

    let request = load_request(&s, request_id).await?;
    let signers = load_signers(&s, request_id).await?;
    Ok(Json(RequestView { request, signers }))
}

#[derive(Serialize)]
pub struct VerifyResult {
    pub status: String,
    /// True if the document is byte-identical to when it was signed (no tampering).
    pub intact: bool,
    pub doc_hash: Option<String>,
    pub current_hash: Option<String>,
    pub signers: Vec<SignerView>,
}

/// GET /fh/api/sign-requests/:id/verify — integrity + signer audit.
pub async fn verify(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(request_id): Path<Uuid>,
) -> ApiResult<Json<VerifyResult>> {
    let req = load_request(&s, request_id).await?;
    access_or_404(&s, &user, req.file_id).await?;
    let stored: Option<(Option<String>,)> =
        sqlx::query_as("SELECT doc_hash FROM signature_requests WHERE id = $1")
            .bind(request_id)
            .fetch_optional(&s.db)
            .await?;
    let doc_hash_val = stored.and_then(|r| r.0);
    let current = doc_hash(&s, req.file_id).await;
    let intact = match (&doc_hash_val, &current) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    let signers = load_signers(&s, request_id).await?;
    Ok(Json(VerifyResult {
        status: req.status,
        intact,
        doc_hash: doc_hash_val,
        current_hash: current,
        signers,
    }))
}

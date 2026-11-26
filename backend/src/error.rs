use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    /// The file's status locks its content (no new version, no rename).
    #[error("content locked: {0}")]
    ContentLocked(String),
    /// Not allowed, with the reason people should see.
    #[error("denied: {0}")]
    Denied(String),
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    /// The client stopped sending (408).
    #[error("timeout: {0}")]
    Timeout(String),
    /// Overloaded right now; the client should retry (503 + Retry-After).
    #[error("busy: {0}")]
    Busy(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("too many requests: {0}")]
    TooManyRequests(String),
    /// A refusal with its own status, machine code and sentence.
    #[error("{2}")]
    Coded(StatusCode, &'static str, String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            ApiError::NotFound          => (StatusCode::NOT_FOUND,           "This item doesn't exist or you don't have access to it.".to_string()),
            ApiError::BadRequest(m)     => (StatusCode::BAD_REQUEST,         m.clone()),
            ApiError::Unauthorized      => (StatusCode::UNAUTHORIZED,        "Your session has ended. Sign in again.".to_string()),
            ApiError::Forbidden         => (StatusCode::FORBIDDEN,           "You don't have access to do this.".to_string()),
            ApiError::Denied(m)         => (StatusCode::FORBIDDEN,           m.clone()),
            ApiError::PayloadTooLarge(m)=> (StatusCode::PAYLOAD_TOO_LARGE,   m.clone()),
            ApiError::Conflict(m)       => (StatusCode::CONFLICT,            m.clone()),
            ApiError::ContentLocked(m)  => (StatusCode::CONFLICT,            m.clone()),
            ApiError::Timeout(m)        => (StatusCode::REQUEST_TIMEOUT,     m.clone()),
            ApiError::Coded(st, code, m) => {
                return (*st, Json(json!({ "error": m, "code": code }))).into_response();
            }
            ApiError::Busy(m) => {
                return (StatusCode::SERVICE_UNAVAILABLE, [(axum::http::header::RETRY_AFTER, "5")], Json(json!({ "error": m }))).into_response();
            }
            ApiError::TooManyRequests(m)=> {
                // Limits use one-minute windows: wait until the next minute.
                let secs = (60 - chrono::Utc::now().timestamp().rem_euclid(60)).max(1);
                return (StatusCode::TOO_MANY_REQUESTS, [(axum::http::header::RETRY_AFTER, secs.to_string())],
                    Json(json!({ "error": m, "code": "rate_limited", "retry_after": secs }))).into_response();
            }
            ApiError::Db(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "This item doesn't exist or you don't have access to it.".to_string()),
            // Database unreachable or saturated: temporary — 503 so clients and
            // load balancers retry, not a server bug.
            ApiError::Db(e @ (sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) | sqlx::Error::PoolClosed)) => {
                tracing::warn!("database unavailable: {e}");
                return (StatusCode::SERVICE_UNAVAILABLE, [(axum::http::header::RETRY_AFTER, "5")],
                    Json(json!({ "error": "The service is busy or restarting. Try again in a moment.", "code": "unavailable" }))).into_response();
            }
            ApiError::Db(e) => {
                tracing::error!("db error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong on the server. Try again.".to_string())
            }
            ApiError::Other(e) => {
                tracing::error!("internal error: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong on the server. Try again.".to_string())
            }
        };
        // `code`: stable for programs; `error`: a sentence when there is one.
        (status, Json(json!({ "error": msg, "code": self.code() }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    /// Stable machine-readable name of the error kind.
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::NotFound => "not_found",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Unauthorized => "unauthorized",
            ApiError::Forbidden | ApiError::Denied(_) => "forbidden",
            ApiError::PayloadTooLarge(_) => "too_large",
            ApiError::TooManyRequests(_) => "rate_limited",
            ApiError::Busy(_) => "busy",
            ApiError::Timeout(_) => "timeout",
            ApiError::Conflict(_) => "conflict",
            ApiError::ContentLocked(_) => "content_locked",
            ApiError::Coded(_, c, _) => c,
            ApiError::Db(sqlx::Error::RowNotFound) => "not_found",
            ApiError::Db(_) | ApiError::Other(_) => "internal",
        }
    }
}

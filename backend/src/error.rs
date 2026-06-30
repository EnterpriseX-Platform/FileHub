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
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    #[error("too many requests: {0}")]
    TooManyRequests(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            ApiError::NotFound          => (StatusCode::NOT_FOUND,           "not found".to_string()),
            ApiError::BadRequest(m)     => (StatusCode::BAD_REQUEST,         m.clone()),
            ApiError::Unauthorized      => (StatusCode::UNAUTHORIZED,        "unauthorized".to_string()),
            ApiError::Forbidden         => (StatusCode::FORBIDDEN,           "forbidden".to_string()),
            ApiError::Conflict(m)       => (StatusCode::CONFLICT,            m.clone()),
            ApiError::PayloadTooLarge(m)=> (StatusCode::PAYLOAD_TOO_LARGE,   m.clone()),
            ApiError::TooManyRequests(m)=> (StatusCode::TOO_MANY_REQUESTS,   m.clone()),
            ApiError::Db(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "not found".to_string()),
            ApiError::Db(e) => {
                tracing::error!("db error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "db error".to_string())
            }
            ApiError::Other(e) => {
                tracing::error!("internal error: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

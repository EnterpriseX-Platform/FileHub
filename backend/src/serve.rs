//! Streaming file responses shared by every download path (`/fh/api/files/:id/download`,
//! share links, legacy `/FileService/downloadFile|previewFile`).
//!
//! The body is streamed straight from the store (decrypting chunk by chunk
//! for format-v2 objects), so memory per request no longer grows with the
//! file size, and `Range:` is honoured everywhere — `<video>` seeking and
//! resumable downloads work on every path, including the legacy one that
//! most NEB modules call.

use std::sync::OnceLock;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// `Content-Disposition` that survives any file name — ASCII fallback plus
/// RFC 5987 `filename*` so Thai names are kept (and a `"` can't break it).
pub fn content_disposition(kind: &str, name: &str) -> HeaderValue {
    let ascii = crate::handlers::sanitize_filename(name);
    let utf8 = urlencoding::encode(name);
    HeaderValue::from_str(&format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{utf8}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// Parse a single-range `bytes=start-end` header into an inclusive `(start, end)`
/// pair, clamped to `total`.  `None` for multipart ranges, other units, or an
/// unsatisfiable range.
pub fn parse_range(h: &str, total: u64) -> Option<(u64, u64)> {
    let rest = h.trim().strip_prefix("bytes=")?;
    if rest.contains(',') { return None; }
    let (s, e) = rest.split_once('-')?;
    let (s, e) = (s.trim(), e.trim());
    if total == 0 { return None; }
    let (start, end) = if s.is_empty() {
        let suffix: u64 = e.parse().ok()?;
        if suffix == 0 { return None; }
        (total.saturating_sub(suffix), total - 1)
    } else {
        let start: u64 = s.parse().ok()?;
        let end: u64 = if e.is_empty() { total - 1 } else { e.parse().ok()? };
        (start, end.min(total - 1))
    };
    if start > end || start >= total { return None; }
    Some((start, end))
}

pub struct ServeOpts<'a> {
    pub key: &'a str,
    pub encrypted: bool,
    pub name: &'a str,
    pub etag: Option<&'a str>,
    /// `inline` or `attachment`
    pub disposition: &'a str,
    pub cache_control: &'a str,
}

/// Build a streamed 200 / 206 / 416 response for a stored object.
pub async fn stream_object(s: &AppState, o: ServeOpts<'_>, req: &HeaderMap) -> ApiResult<Response> {
    let info = s.storage.stat(o.key, o.encrypted).await?.ok_or(ApiError::NotFound)?;
    let total = info.size;
    let ct = mime_guess::from_path(o.name).first()
        .map(|m| m.to_string())
        .unwrap_or_else(|| "application/octet-stream".into());

    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&ct).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    h.insert(header::CONTENT_DISPOSITION, content_disposition(o.disposition, o.name));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Ok(v) = HeaderValue::from_str(o.cache_control) { h.insert(header::CACHE_CONTROL, v); }
    if let Some(etag) = o.etag {
        if let Ok(v) = HeaderValue::from_str(&format!("\"{etag}\"")) { h.insert(header::ETAG, v); }
    }

    // If-Range: only honour the range when the validator still matches.
    let if_range_ok = match (req.get(header::IF_RANGE).and_then(|v| v.to_str().ok()), o.etag) {
        (None, _) => true,
        (Some(v), Some(etag)) => v.trim().trim_start_matches("W/").trim_matches('"') == etag,
        (Some(_), None) => false,
    };
    let range = req.get(header::RANGE).and_then(|v| v.to_str().ok()).filter(|_| if_range_ok);

    let (status, start, end) = match range {
        Some(r) => match parse_range(r, total) {
            Some((a, b)) => (StatusCode::PARTIAL_CONTENT, a, b),
            None => {
                h.insert(header::CONTENT_RANGE, HeaderValue::from_str(&format!("bytes */{total}")).unwrap());
                return Ok((StatusCode::RANGE_NOT_SATISFIABLE, h).into_response());
            }
        },
        None => (StatusCode::OK, 0, total.saturating_sub(1)),
    };
    let len = if total == 0 { 0 } else { end - start + 1 };
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    if status == StatusCode::PARTIAL_CONTENT {
        h.insert(header::CONTENT_RANGE, HeaderValue::from_str(&format!("bytes {start}-{end}/{total}")).unwrap());
    }
    let body = if len == 0 { Body::empty() } else {
        Body::from_stream(s.storage.read_range(o.key, &info, start, end).await?)
    };
    Ok((status, h, body).into_response())
}

/// One HTTP client for calls to the old file hub — connection reuse plus
/// timeouts (the old code built a new client per request with none).
pub fn upstream_client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("reqwest client")
    })
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=50-500", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=5-1", 100), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        assert_eq!(parse_range("bytes=-0", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
    }
}

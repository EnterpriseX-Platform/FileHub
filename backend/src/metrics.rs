//! Prometheus metrics at `/fh/metrics` (text format), for Grafana alerts on
//! errors, latency, the upload queue and the database pool.
//!
//! Off unless `METRICS_TOKEN` is set; the scraper then sends
//! `Authorization: Bearer <token>`. Counters are per pod (Prometheus sums pods).

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Instant;

use axum::{extract::State, http::{header, HeaderMap, StatusCode}, response::{IntoResponse, Response}};

use crate::state::AppState;

const KINDS: [&str; 3] = ["upload", "download", "other"];
const CLASSES: [&str; 3] = ["2xx", "4xx", "5xx"];
/// Latency bucket upper bounds, seconds.
const BUCKETS: [f64; 10] = [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

struct Counters {
    requests: [[AtomicU64; 3]; 3],            // [kind][class]
    latency: [[AtomicU64; BUCKETS.len() + 1]; 3], // [kind][bucket], last = +Inf
    latency_sum_us: [AtomicU64; 3],
}

fn counters() -> &'static Counters {
    static C: std::sync::OnceLock<Counters> = std::sync::OnceLock::new();
    C.get_or_init(|| Counters {
        requests: Default::default(),
        latency: Default::default(),
        latency_sum_us: Default::default(),
    })
}

fn kind_of(method: &axum::http::Method, path: &str) -> usize {
    if crate::admission::is_upload(method, path) { 0 }
    else if path.contains("/download") || path.starts_with("/FileService/downloadFile") || path.contains("/preview") { 1 }
    else { 2 }
}

/// Middleware: count every request and its duration.
pub async fn track(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let kind = kind_of(req.method(), req.uri().path());
    let started = Instant::now();
    let res = next.run(req).await;
    let secs = started.elapsed().as_secs_f64();
    let class = match res.status().as_u16() { 500.. => 2, 400.. => 1, _ => 0 };
    let c = counters();
    c.requests[kind][class].fetch_add(1, Relaxed);
    let b = BUCKETS.iter().position(|ub| secs <= *ub).unwrap_or(BUCKETS.len());
    c.latency[kind][b].fetch_add(1, Relaxed);
    c.latency_sum_us[kind].fetch_add((secs * 1e6) as u64, Relaxed);
    res
}

/// GET /fh/metrics
pub async fn endpoint(State(s): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Ok(token) = std::env::var("METRICS_TOKEN") else { return StatusCode::NOT_FOUND.into_response() };
    let given = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if token.is_empty() || !crate::config::constant_eq(given.unwrap_or("").as_bytes(), token.as_bytes()) { return StatusCode::UNAUTHORIZED.into_response(); }

    let c = counters();
    let mut out = String::new();
    out.push_str("# HELP filehub_http_requests_total Requests by kind and status class.\n# TYPE filehub_http_requests_total counter\n");
    for (k, kind) in KINDS.iter().enumerate() {
        for (i, class) in CLASSES.iter().enumerate() {
            out.push_str(&format!("filehub_http_requests_total{{kind=\"{kind}\",status=\"{class}\"}} {}\n", c.requests[k][i].load(Relaxed)));
        }
    }
    out.push_str("# HELP filehub_http_request_duration_seconds Request duration.\n# TYPE filehub_http_request_duration_seconds histogram\n");
    for (k, kind) in KINDS.iter().enumerate() {
        let mut cumulative = 0;
        for (b, ub) in BUCKETS.iter().enumerate() {
            cumulative += c.latency[k][b].load(Relaxed);
            out.push_str(&format!("filehub_http_request_duration_seconds_bucket{{kind=\"{kind}\",le=\"{ub}\"}} {cumulative}\n"));
        }
        cumulative += c.latency[k][BUCKETS.len()].load(Relaxed);
        out.push_str(&format!("filehub_http_request_duration_seconds_bucket{{kind=\"{kind}\",le=\"+Inf\"}} {cumulative}\n"));
        out.push_str(&format!("filehub_http_request_duration_seconds_sum{{kind=\"{kind}\"}} {}\n", c.latency_sum_us[k].load(Relaxed) as f64 / 1e6));
        out.push_str(&format!("filehub_http_request_duration_seconds_count{{kind=\"{kind}\"}} {cumulative}\n"));
    }
    let (waiting, writing) = crate::admission::queue_depth();
    out.push_str(&format!("# HELP filehub_indexing_backlog Uploads waiting for text/thumbnail/preview.\n# TYPE filehub_indexing_backlog gauge\nfilehub_indexing_backlog {}\n", crate::jobs::indexing_backlog()));
    out.push_str(&format!("# HELP filehub_uploads_waiting Upload requests waiting for a turn.\n# TYPE filehub_uploads_waiting gauge\nfilehub_uploads_waiting {waiting}\n"));
    out.push_str(&format!("# HELP filehub_uploads_writing Uploads in their database phase.\n# TYPE filehub_uploads_writing gauge\nfilehub_uploads_writing {writing}\n"));
    out.push_str("# HELP filehub_uploads_in_step Admitted uploads by step.\n# TYPE filehub_uploads_in_step gauge\n");
    for p in crate::admission::Phase::ALL { out.push_str(&format!("filehub_uploads_in_step{{step=\"{}\"}} {}\n", p.name(), p.count())); }
    out.push_str(&format!("# HELP filehub_db_pool_connections Pooled database connections.\n# TYPE filehub_db_pool_connections gauge\nfilehub_db_pool_connections{{state=\"open\"}} {}\nfilehub_db_pool_connections{{state=\"idle\"}} {}\n",
        s.db.size(), s.db.num_idle()));
    let files = sqlx::query_scalar::<_, i64>("SELECT reltuples::bigint FROM pg_class WHERE relname = 'files'").fetch_one(&s.db).await;
    let bytes = crate::handlers::cached_sum(&s.db, "workspace", "SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM files WHERE deleted_at IS NULL", None).await;
    if let (Ok(files), Ok(bytes)) = (files, bytes) {
        out.push_str(&format!("# HELP filehub_files Files stored (estimate).\n# TYPE filehub_files gauge\nfilehub_files {files}\n"));
        out.push_str(&format!("# HELP filehub_stored_bytes Bytes stored in live files.\n# TYPE filehub_stored_bytes gauge\nfilehub_stored_bytes {bytes}\n"));
    }
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out).into_response()
}

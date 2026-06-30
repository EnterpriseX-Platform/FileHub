//! Library crate so integration tests in `tests/` can import the same
//! handlers + storage + models the binary uses.  The CLI entrypoint lives in
//! `main.rs` and calls into this crate.

pub mod ai;
pub mod ai_api;
pub mod ai_worker;
pub mod auth;
pub mod checkout;
pub mod error;
pub mod handlers;
pub mod models;
pub mod p1;
pub mod reports;
pub mod rotation;
pub mod seed_demo;
pub mod state;
pub mod storage;
pub mod store;
pub mod tus;
pub mod wopi;

use std::sync::Arc;

use axum::{extract::DefaultBodyLimit, middleware, routing::get, Router};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::Span;

pub use state::AppState;

/// Construct the application router wrapped around an already-built
/// `AppState`.  Used by both the production binary and the integration tests
/// so the routes never drift between them.
///
/// All routes are mounted under the `/fh` URL prefix so the API can sit
/// behind a reverse proxy alongside the `/filehub` frontend without colliding
/// with neighbouring services.
///
/// Routes are split into two sub-routers so a single `require_session`
/// middleware layer covers every authenticated endpoint at once.  The
/// alternative — adding `_user: AuthUser` to every handler — was the
/// previous approach and it failed: ~half the read handlers (list_files,
/// stats, …) had no extractor and served data anonymously.  A layer can't
/// be forgotten when adding a new route.
///
/// Public (no session required):
///   - `/api/auth/login`, `/api/auth/logout`, `/api/auth/me`  (login flow;
///     `me` self-enforces auth via the `AuthUser` extractor)
///   - `/api/health`                                          (liveness probe)
///   - `/api/share/:token`, `/api/share/:token/download`      (the token IS
///     the credential — share-link recipients aren't logged in)
///   - `/wopi/*`                                              (Collabora
///     authenticates via the `access_token` query param we mint per file)
pub fn build_router(state: Arc<AppState>) -> Router {
    // `CORS_ORIGIN` is the single browser-facing origin that may call this
    // API.  We refuse to silently fall back to `localhost:3001` in release
    // because a misconfigured prod deploy would otherwise quietly accept
    // dev-origin credentials only.  Debug builds keep the fallback so
    // `cargo test` continues to work without bespoke env setup.
    let cors_origin = match std::env::var("CORS_ORIGIN") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            #[cfg(debug_assertions)] { "http://localhost:3001".to_string() }
            #[cfg(not(debug_assertions))] {
                panic!("CORS_ORIGIN must be set in release builds")
            }
        }
    };
    let allow_origin = cors_origin.parse::<axum::http::HeaderValue>()
        .unwrap_or_else(|e| panic!("CORS_ORIGIN={cors_origin:?} is not a valid header value: {e}"));
    // Cookie auth requires `credentials: 'include'` from the browser, which
    // disallows `Any` for origin/headers — we mirror the configured origin
    // exactly and allow credentials.  `Cookie` is NOT in `allow_headers` —
    // browsers forbid scripts from setting it anyway, listing it just
    // implies (incorrectly) that scripted cookie spoofing is possible.
    let cors = CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
            axum::http::header::IF_NONE_MATCH,
            // TUS-protocol headers — browsers reject preflight for these
            // unless explicitly allowed.
            axum::http::HeaderName::from_static("tus-resumable"),
            axum::http::HeaderName::from_static("upload-length"),
            axum::http::HeaderName::from_static("upload-offset"),
            axum::http::HeaderName::from_static("upload-metadata"),
        ])
        .expose_headers([
            axum::http::header::LOCATION,
            axum::http::header::ETAG,
            axum::http::HeaderName::from_static("tus-resumable"),
            axum::http::HeaderName::from_static("upload-offset"),
            axum::http::HeaderName::from_static("upload-length"),
            // Grand total for the current /api/files filter, so the browser can paginate.
            axum::http::HeaderName::from_static("x-total-count"),
        ]);

    // Public routes — no session cookie required.  The token / health probe
    // /me extractor self-gate is sufficient.
    let public: Router<Arc<AppState>> = Router::new()
        .route("/api/auth/login",             axum::routing::post(auth::login))
        .route("/api/auth/logout",            axum::routing::post(auth::logout))
        .route("/api/auth/me",                get(auth::me))
        .route("/api/health",                 get(handlers::health))
        // Deep readiness — probes DB + storage. Heavier than /health but
        // still cheap; safe to hit on every k8s readinessProbe tick.
        .route("/api/ready",                  get(handlers::ready))
        // API docs — public (the spec has no secrets). Raw spec is embedded at
        // build time; Swagger UI is served at /fh/docs and reads it.
        .route("/api/openapi.yaml",           get(handlers::openapi_spec))
        .route("/docs",                       get(handlers::swagger_ui))
        // Share-link endpoints carry a one-shot token in the URL — that
        // token IS the credential, so the recipient doesn't need a session.
        .route("/api/share/:token",           get(handlers::share_meta))
        .route("/api/share/:token/download",  get(handlers::share_download))
        // WOPI endpoints — Collabora calls these with `?access_token=...`
        // which we mint per-file in `wopi::office_url`.  Token validation
        // happens inside the handlers.
        .route("/wopi/files/:id",             get(wopi::check_file_info))
        .route("/wopi/files/:id/contents",    get(wopi::get_file).post(wopi::put_file));

    // Private routes — every request must carry a valid `filehub_session`
    // cookie.  The middleware rejects anonymous callers with 401 before any
    // handler runs.
    let private: Router<Arc<AppState>> = Router::new()
        .route("/api/stats",                  get(handlers::stats))
        .route(
            "/api/systems",
            get(handlers::list_systems).post(handlers::create_system),
        )
        .route(
            "/api/systems/:id",
            axum::routing::patch(handlers::patch_system)
                .delete(handlers::delete_system),
        )
        .route("/api/personal-drive",         get(handlers::personal_drive))
        .route("/api/orgs",                   get(handlers::list_orgs))
        .route("/api/orgs/:id/quota",         axum::routing::patch(handlers::patch_org_quota))
        // Q1/Q5 — workspace branding + access-policy toggles
        .route(
            "/api/workspace",
            get(handlers::list_workspace_config)
                .patch(handlers::patch_workspace),
        )
        // Q2 — members CRUD (admin role gates POST/PATCH; list visible to editors)
        .route(
            "/api/users",
            get(handlers::list_members)
                .post(handlers::create_member),
        )
        .route(
            "/api/users/:id",
            axum::routing::patch(handlers::patch_member),
        )
        .route("/api/users/:id/quota",        axum::routing::patch(handlers::patch_user_quota))
        .route("/api/quota",                  get(handlers::quota_report))
        // API keys (machine-to-machine) — admin-managed. The keys themselves
        // authenticate via `Authorization: Bearer <key>` on any private route
        // (see auth.rs::resolve_api_key), so an integration can upload headless.
        .route("/api/api-keys",               get(handlers::list_api_keys).post(handlers::create_api_key))
        .route("/api/api-keys/:id",           axum::routing::delete(handlers::revoke_api_key))
        // Phase K — rotation policies + runs
        .route(
            "/api/rotation/policies",
            get(handlers::list_rotation_policies)
                .post(handlers::upsert_rotation_policy),
        )
        .route(
            "/api/rotation/policies/:id",
            axum::routing::delete(handlers::delete_rotation_policy),
        )
        .route("/api/rotation/run",  axum::routing::post(handlers::run_rotation_now))
        .route("/api/rotation/runs", get(handlers::list_rotation_runs))
        // Phase U — Collabora Online: the *minter* needs a session (we issue
        // an access_token bound to the caller's identity).  The WOPI host
        // endpoints themselves are public — see above.
        .route("/api/files/:id/office-url",   get(wopi::office_url))
        .route("/api/search",                 get(handlers::search_files))
        // AI-native: meaning-based search (pgvector kNN), permission-scoped.
        .route("/api/search/semantic",        axum::routing::post(ai_api::semantic_search))
        // AI-native: RAG — grounded, cited, permission-aware answer.
        .route("/api/ask",                    axum::routing::post(ai_api::ask))
        // File CRUD lives in its own sub-router so we can raise the body
        // limit on the multipart endpoints (upload, batch, new version, and
        // patch which may rewrite bytes) without affecting JSON-only routes.
        // axum's default extractor cap is 2 MiB; the frontend allows up to
        // 64 MiB per file before falling through to the resumable TUS path.
        .merge(
            Router::new()
                .route("/api/files",          get(handlers::list_files).post(handlers::upload_file))
                .route("/api/files/batch",    axum::routing::post(handlers::upload_batch))
                .route(
                    "/api/files/:id",
                    get(handlers::file_detail)
                        .patch(handlers::patch_file)
                        .delete(handlers::delete_file),
                )
                .route(
                    "/api/files/:id/versions",
                    get(handlers::list_versions).post(handlers::upload_version),
                )
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        )
        .route("/api/files/:id/download",     get(handlers::download_file))
        .route("/api/files/:id/share",        axum::routing::post(handlers::create_share_link))
        .route("/api/folders",                get(handlers::list_folders).post(handlers::create_folder))
        .route("/api/folders/:id",            axum::routing::delete(handlers::delete_folder))
        .route("/api/trash",                  get(handlers::list_trash))
        .route("/api/files/:id/restore",      axum::routing::post(handlers::restore_file))
        // P1 — workflow / comments / notifications / thumbnails
        .route("/api/files/:id/thumb",        get(p1::get_thumbnail))
        .route("/api/files/:id/preview",      get(p1::get_preview))
        // TUS resumable upload (creation + termination extensions).
        // Mounted as a sub-router so we can raise the body limit on the PATCH
        // chunk endpoint without affecting any other route.  Axum's default
        // Bytes-extractor limit is 2 MiB; the frontend streams 8 MiB chunks,
        // and we leave headroom for proxies that might buffer slightly more.
        .merge(
            Router::new()
                .route("/api/uploads",       axum::routing::options(tus::options_capabilities)
                                               .post(tus::create_session))
                .route("/api/uploads/:id",   axum::routing::head(tus::head_session)
                                               .patch(tus::append_chunk)
                                               .delete(tus::terminate_session))
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        )
        .route("/api/files/:id/workflow",
            get(p1::list_workflow).post(p1::start_workflow))
        .route("/api/workflow-steps/:id/decision",
            axum::routing::post(p1::decide_step))
        .route("/api/files/:id/comments",
            get(p1::list_comments).post(p1::create_comment))
        .route("/api/files/:id/comments/:comment_id",
            axum::routing::delete(p1::delete_comment))
        .route("/api/files/:id/extras",       get(p1::file_extras))
        // AI-native: per-file summary / tags / sensitivity + enrichment status.
        .route("/api/files/:id/ai",           get(ai_api::file_ai))
        // Check-out / check-in locking (TOR 5.3.8.4-5).
        .route("/api/files/:id/lock",         get(checkout::get_lock))
        .route("/api/files/:id/checkout",     axum::routing::post(checkout::checkout))
        .route("/api/files/:id/checkin",      axum::routing::post(checkout::checkin))
        .route("/api/notifications",          get(p1::list_notifications))
        .route("/api/notifications/unread-count", get(p1::unread_count))
        .route("/api/notifications/:id/read", axum::routing::post(p1::mark_notification_read))
        .route("/api/reports/by-category",    get(handlers::report_by_category))
        .route("/api/reports/by-time",        get(handlers::report_by_time))
        // Document status report + exports (TOR 5.3.1.16, 5.3.7.6).
        .route("/api/reports/status",         get(reports::status_report))
        .route("/api/reports/status.csv",     get(reports::status_csv))
        .route("/api/activity/export.csv",    get(reports::audit_csv))
        .route("/api/activity",               get(handlers::list_activity))
        .route("/api/views",                  get(handlers::list_views).post(handlers::create_view))
        .route("/api/permissions/:file_id",   get(handlers::list_permissions))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_session));

    // Structured access log: one INFO line per response carrying the method,
    // path, status, and wall-clock latency.  `on_request` is silenced (the
    // default span already records method/uri at DEBUG) so each request emits
    // exactly one line at INFO — cheap enough to leave on in production and
    // enough to drive request-rate / error-rate / p99-latency dashboards.
    let trace = TraceLayer::new_for_http()
        .on_request(())
        .on_response(|res: &axum::http::Response<_>, latency: std::time::Duration, span: &Span| {
            let _e = span.enter();
            tracing::info!(
                status = res.status().as_u16(),
                latency_ms = latency.as_millis() as u64,
                "request completed"
            );
        });

    let inner = public
        .merge(private)
        .with_state(state)
        .layer(cors)
        .layer(trace);

    Router::new().nest("/fh", inner)
}

#![recursion_limit = "256"]
//! Library crate so integration tests in `tests/` can import the same
//! handlers + storage + models the binary uses.  The CLI entrypoint lives in
//! `main.rs` and calls into this crate.

pub mod acl;
pub mod id;
pub mod scan;
pub mod origin;
pub use id::{is_id, new_id, Id};
pub mod admission;
pub mod config;
pub mod auth;
pub mod basket;
pub mod error;
pub mod handlers;
pub mod jobs;
pub mod jwks;
pub mod layout;
pub mod connectors;
pub mod legacy;
pub mod mount;
pub mod legacy_import;
pub mod metrics;
pub mod models;
pub mod objectlock;
pub mod p1;
pub mod report_export;
pub mod rotation;
pub mod s3;
pub mod s3import;
pub mod seed_demo;
pub mod serve;
pub mod ratelimit;
pub mod sharelink;
pub mod stars;
pub mod stamp;
pub mod state;
pub mod storage;
pub mod store;
pub mod tus;
pub mod uploadlink;
pub mod vbasket;
pub mod webhooks;
pub mod wopi;

use std::sync::Arc;

use axum::{extract::DefaultBodyLimit, middleware, routing::get, Router};
use tower_http::{cors::{Any, CorsLayer}, trace::TraceLayer};
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
/// Pitfall: this layer must wrap the Router from the OUTSIDE, in `main.rs` only —
/// axum's `Router::layer` runs **after** route matching ⇒ applied here, the
/// request would already get a 405 before we could rewrite the method.
/// (This has been debugged once already — do not move it back.)
///
/// Some edge proxies (e.g. a CDN/WAF) only let GET and POST through —
/// PATCH/PUT/DELETE/HEAD/OPTIONS are answered with a 403 HTML page before the
/// request reaches the app ⇒ deleting files, editing file metadata, managing
/// users and quotas would all be unusable from the web UI.
///
/// Reconfiguring the WAF is not always possible, so clients send POST and carry
/// the real method in the `X-HTTP-Method-Override` header. This layer wraps the
/// whole router and runs BEFORE route matching ⇒ every existing endpoint works
/// unchanged, no duplicate routes are needed, and callers that send the real
/// method directly (e.g. from inside the cluster) keep working as before.
pub async fn method_override(
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if req.method() == axum::http::Method::POST {
        let want = req
            .headers()
            .get("x-http-method-override")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_ascii_uppercase());
        if let Some(w) = want {
            // Only allow the methods an edge proxy would block — not arbitrary overrides.
            let m = match w.as_str() {
                "DELETE" => Some(axum::http::Method::DELETE),
                "PATCH" => Some(axum::http::Method::PATCH),
                "PUT" => Some(axum::http::Method::PUT),
                // tus asks for the current upload offset with HEAD (on resume/retry).
                "HEAD" => Some(axum::http::Method::HEAD),
                _ => None,
            };
            if let Some(m) = m {
                *req.method_mut() = m;
            }
        }
    }
    next.run(req).await
}

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
            axum::http::Method::PUT,
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
            axum::http::HeaderName::from_static("x-http-method-override"),
        ])
        .expose_headers([
            axum::http::header::LOCATION,
            axum::http::header::ETAG,
            axum::http::HeaderName::from_static("tus-resumable"),
            axum::http::HeaderName::from_static("upload-offset"),
            axum::http::HeaderName::from_static("upload-length"),
            axum::http::HeaderName::from_static("upload-file-id"),
            // Grand total for the current /api/files filter, so the browser can paginate.
            axum::http::HeaderName::from_static("x-total-count"),
        ]);

    // Public routes — no session cookie required.  The token / health probe
    // /me extractor self-gate is sufficient.
    let public: Router<Arc<AppState>> = Router::new()
        .route("/api/auth/login",             axum::routing::post(auth::login))
        .route("/api/auth/signup",            axum::routing::post(auth::signup))
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
        .route("/api/branding",               get(handlers::branding))
        .route("/api/share/:token",           get(sharelink::meta))
        .route("/api/share/:token/download",  get(sharelink::download_get).post(sharelink::download_post))
        .route("/api/share/:token/view",      get(sharelink::view_get).post(sharelink::view_post))
        // Upload links: the token is the credential; files land in one folder.
        .merge(
            Router::new()
                .route("/api/upload/:token", get(uploadlink::public_meta).post(uploadlink::public_upload))
                .layer(DefaultBodyLimit::max(crate::handlers::upload_max_bytes() as usize))
        )
        .merge(
            Router::new()
                .route("/api/upload/:token/chunks",     axum::routing::options(tus::options_capabilities).post(tus::link_create))
                .route("/api/upload/:token/chunks/:id", axum::routing::head(tus::link_head)
                                               .patch(tus::link_patch)
                                               .delete(tus::link_terminate))
                .layer(DefaultBodyLimit::max(crate::tus::chunk_max_bytes()))
        )
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
        .route("/api/legacy-import", axum::routing::post(legacy_import::import_from_legacy))
        .route("/api/rotation/run",  axum::routing::post(handlers::run_rotation_now))
        .route("/api/rotation/runs", get(handlers::list_rotation_runs))
        // Phase U — Collabora Online: the *minter* needs a session (we issue
        // an access_token bound to the caller's identity).  The WOPI host
        // endpoints themselves are public — see above.
        .route("/api/files/:id/office-url",   get(wopi::office_url))
        .route("/api/search",                 get(handlers::search_files))
        .route("/api/search/folders",         get(handlers::search_folders))
        .route("/api/tags",                   get(handlers::list_tags))
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
                .layer(DefaultBodyLimit::max(crate::handlers::upload_max_bytes() as usize))
        )
        .route("/api/files/:id/download",     get(handlers::download_file))
        .route("/api/files/:id/share",        axum::routing::post(sharelink::create))
        .route("/api/files/:id/links",        get(sharelink::list))
        .route("/api/share-links/:id",        axum::routing::delete(sharelink::revoke))
        .route("/api/baskets/:id/share-links", get(sharelink::list_for_basket))
        .route("/api/folders",                get(handlers::list_folders).post(handlers::create_folder))
        .route("/api/folders/:id",            get(basket::folder_detail).delete(handlers::delete_folder).patch(basket::patch_folder))
        .route("/api/folders/:id/zip",        get(vbasket::folder_zip))
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
                .layer(DefaultBodyLimit::max(crate::tus::chunk_max_bytes()))
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
        .route("/api/notifications",          get(p1::list_notifications))
        .route("/api/notifications/unread-count", get(p1::unread_count))
        .route("/api/notifications/:id/read", axum::routing::post(p1::mark_notification_read))
        .route("/api/reports/by-category",    get(handlers::report_by_category))
        .route("/api/reports/by-time",        get(handlers::report_by_time))
        .route("/api/reports/export",         get(report_export::export))
        .route("/api/activity",               get(handlers::list_activity))
        .route("/api/views",                  get(handlers::list_views).post(handlers::create_view))
        .route("/api/views/:id",              axum::routing::delete(handlers::delete_view))
        .route("/api/permissions/:file_id",   get(handlers::list_permissions))
        // Basket platform (phase 1): settings, access, status, move
        .route("/api/baskets/:id/settings",   get(basket::get_settings).put(basket::put_settings))
        .route("/api/acl/:rtype/:rid",        get(acl::get_acl).put(acl::put_acl))
        .route("/api/acl/:rtype/:rid/effective", get(acl::effective))
        .route("/api/principals",             get(acl::search_principals))
        .route("/api/files/:id/origin",       get(origin::get))
        .route("/api/files/:id/status",       get(basket::next_statuses).post(basket::change_status))
        .route("/api/move",                   axum::routing::post(basket::move_items))
        .route("/api/folders/:id/policy",     get(basket::get_folder_policy).put(basket::put_folder_policy))
        .route("/api/files/:id/encrypt",      axum::routing::post(basket::encrypt_file))
        // Virtual Baskets + download list (phase 2)
        .route("/api/vbaskets",               get(vbasket::list).post(vbasket::create))
        .route("/api/vbaskets/preview",       axum::routing::post(vbasket::preview))
        .route("/api/vbaskets/fields",        get(vbasket::fields))
        .route("/api/vbaskets/:id",           get(vbasket::get_one).put(vbasket::update).delete(vbasket::remove))
        .route("/api/vbaskets/:id/tree",      get(vbasket::tree))
        .route("/api/vbaskets/:id/excludes",  axum::routing::post(vbasket::exclude))
        .route("/api/vbaskets/:id/excludes/:file_id", axum::routing::delete(vbasket::unexclude))
        .route("/api/files/:id/appears-in",   get(vbasket::appears_in))
        .route("/api/files/:id/activity",     get(handlers::file_activity))
        .route("/api/me/shared",              get(acl::shared_with_me))
        .route("/api/baskets/:id/mount",       axum::routing::put(mount::set_mount))
        .route("/api/baskets/:id/scan",        axum::routing::post(mount::scan_now))
        .route("/api/mount-roots",             get(mount::list_roots))
        .route("/api/admin/connectors",        get(connectors::list).post(connectors::create))
        .route("/api/admin/layout",            get(layout::get))
        .route("/api/admin/layout/prepare",    axum::routing::post(layout::post_prepare))
        .route("/api/admin/layout/stages/:year", axum::routing::put(layout::put_stages))
        .route("/api/admin/connectors/:id",    axum::routing::patch(connectors::update).delete(connectors::remove))
        .route("/api/admin/connectors/:id/token", axum::routing::post(connectors::rotate_token))
        .route("/api/admin/import-s3",        axum::routing::post(s3import::start))
        .route("/api/admin/import-s3/:id",    get(s3import::status))
        .route("/api/webhooks",               get(webhooks::list).post(webhooks::create))
        .route("/api/webhooks/:id",           axum::routing::delete(webhooks::remove))
        .route("/api/webhooks/:id/deliveries", get(webhooks::deliveries))
        .route("/api/webhooks/:id/test",      axum::routing::post(webhooks::test))
        .route("/api/files/:id/lock",         get(objectlock::get_lock).put(objectlock::put_lock))
        .route("/api/me/stars",               get(stars::list))
        .route("/api/me/stars/:kind/:id",     axum::routing::put(stars::add).delete(stars::remove))
        .route("/api/upload-links",           get(uploadlink::list).post(uploadlink::create))
        .route("/api/upload-links/:token",    axum::routing::delete(uploadlink::revoke))
        .route("/api/s3-credentials",         get(s3::list_credentials).post(s3::create_credential))
        .route("/api/s3-endpoint",            get(s3::endpoint_info))
        .route("/api/s3-credentials/:ak",     axum::routing::delete(s3::revoke_credential))
        .route("/api/me/review",              get(acl::awaiting_review))
        .route("/api/me/downloads",           get(vbasket::get_downloads).put(vbasket::put_downloads))
        .route("/api/me/downloads/zip",       axum::routing::post(vbasket::download_zip))
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

    let legacy_state = state.clone();
    let metrics_state = state.clone();
    let csrf_origin = cors_origin.clone();
    let inner = public
        .merge(private)
        .with_state(state)
        .layer(cors)
        .layer(middleware::from_fn(move |req, next| csrf_guard(csrf_origin.clone(), req, next)))
        .layer(trace.clone());

    let mut app = Router::new().nest("/fh", inner)
        .layer(middleware::from_fn(security_headers));

    // Compatibility with the legacy FileService API (`/FileService/*`) — off by
    // default; enable with LEGACY_FILESERVICE=1 only where legacy callers must be
    // served (see legacy.rs). It lives outside `/fh` because those callers use this
    // path at the root and their code is not changed.
    // Answers only while a legacy connector is on (Admin → Connectors).
    {
        // The legacy service answered `Access-Control-Allow-Origin: *` ⇒ browsers of
        // legacy apps already calling cross-origin must keep working (no cookies).
        let legacy_cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
        app = app.merge(
            legacy::router()
                .layer(middleware::from_fn_with_state(legacy_state.clone(), legacy::gate))
                .with_state(legacy_state)
                .layer(legacy_cors)
                .layer(trace),
        );
    }

    // Metrics wrap everything, so queueing counts in the latency. Outermost: a
    // bug that panics in one request answers 500; it never takes the pod down.
    app.route("/fh/metrics", axum::routing::get(metrics::endpoint).with_state(metrics_state))
        .layer(middleware::from_fn(metrics::track))
        .layer(tower_http::catch_panic::CatchPanicLayer::custom(|_: Box<dyn std::any::Any + Send>| {
            tracing::error!("request handler panicked — answered 500");
            axum::response::IntoResponse::into_response((axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": "Something went wrong on the server. Try again.", "code": "internal" }))))
        }))
}


/// Headers on every API response: no MIME sniffing (an uploaded file can't be
/// reinterpreted as HTML/script), no framing by other sites, no referrer leak.
/// Cross-site request forgery guard: a browser request that changes something
/// must come from FileHub's own page. Browsers always send `Sec-Fetch-Site`
/// and/or `Origin`; scripts and server-to-server callers send neither and are
/// authenticated by token instead, so they pass.
async fn csrf_guard(allowed: String, req: axum::extract::Request, next: middleware::Next) -> axum::response::Response {
    use axum::{http::Method, response::IntoResponse};
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        let h = req.headers();
        let origin = h.get(axum::http::header::ORIGIN).and_then(|v| v.to_str().ok());
        let site = h.get("sec-fetch-site").and_then(|v| v.to_str().ok());
        let ok = match (origin, site) {
            (Some(o), _) if o == allowed => true,
            (_, Some("same-origin" | "none")) => true,
            (None, None) => true,
            _ => false,
        };
        if !ok {
            return (axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({ "error": "This request didn't come from FileHub.", "code": "forbidden" }))).into_response();
        }
    }
    next.run(req).await
}

async fn security_headers(req: axum::extract::Request, next: middleware::Next) -> axum::response::Response {
    use axum::http::{HeaderName, HeaderValue};
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(HeaderName::from_static("x-content-type-options"), HeaderValue::from_static("nosniff"));
    h.entry(HeaderName::from_static("x-frame-options")).or_insert(HeaderValue::from_static("SAMEORIGIN"));
    h.insert(HeaderName::from_static("referrer-policy"), HeaderValue::from_static("strict-origin-when-cross-origin"));
    res
}

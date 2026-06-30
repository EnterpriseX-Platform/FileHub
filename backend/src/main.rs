use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use filehub_backend::{build_router, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,filehub_backend=debug,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState::init().await?);

    // Phase K — background rotation worker.  Defaults to once per hour; tests
    // and CI can override via `ROTATION_INTERVAL_SECS`.  Disable entirely by
    // setting the env var to `0`.
    //
    // We keep the JoinHandle so the graceful-shutdown path can await the
    // worker — without that, a SIGTERM mid-tick would abort whatever the
    // rotation engine was doing (typically half-finished DB transactions).
    let rotation_secs: u64 = std::env::var("ROTATION_INTERVAL_SECS")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(3600);
    let rotation_handle: Option<JoinHandle<()>> = if rotation_secs > 0 {
        Some(filehub_backend::rotation::spawn_worker(
            Arc::clone(&state),
            std::time::Duration::from_secs(rotation_secs),
        ))
    } else {
        tracing::info!("rotation worker disabled (ROTATION_INTERVAL_SECS=0)");
        None
    };

    // AI enrichment worker — drains the ai_jobs queue (extract → chunk → embed →
    // summarise). Only spawned when AI is enabled; cadence via AI_WORKER_INTERVAL_SECS
    // (default 15s — responsive without busy-looping). See ai_worker.rs.
    let ai_handle: Option<JoinHandle<()>> = if state.ai.enabled() {
        let secs: u64 = std::env::var("AI_WORKER_INTERVAL_SECS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(15);
        Some(filehub_backend::ai_worker::spawn_worker(
            Arc::clone(&state),
            std::time::Duration::from_secs(secs.max(1)),
        ))
    } else {
        tracing::info!("ai enrichment worker disabled (AI_ENABLED=false)");
        None
    };

    let app = build_router(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8090);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("File Hub backend listening on http://0.0.0.0:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // After the HTTP server has stopped accepting connections, give the
    // rotation worker a chance to finish its current tick before we exit.
    // Without this, k8s' default 30-second SIGTERM window would orphan any
    // partially-deleted storage objects mid-loop.
    if let Some(h) = rotation_handle {
        h.abort();
        let _ = h.await;
    }
    if let Some(h) = ai_handle {
        h.abort();
        let _ = h.await;
    }
    Ok(())
}

/// Block until the process receives either SIGINT (Ctrl-C) or SIGTERM (the
/// signal kubernetes uses to drain a pod).  Both kick off the same graceful
/// shutdown path inside axum — finish the in-flight requests, then return.
async fn shutdown_signal() {
    let ctrl_c = async { let _ = tokio::signal::ctrl_c().await; };

    #[cfg(unix)]
    let terminate = async {
        let mut s = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        s.recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c   => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use filehub_backend::{build_router, method_override, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,filehub_backend=debug,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState::init().await?);

    // Staged uploads left by a crash / aborted request (> 1 h old).
    let swept = state.storage.sweep_staging(std::time::Duration::from_secs(3600)).await;
    if swept > 0 { tracing::info!("removed {swept} stale staged upload(s)"); }

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

    // Console admin account, provisioned from the environment's secrets.
    // (The app ships with demo accounts whose passwords are in the public README,
    //  so a real account with an operator-chosen password is needed; it is
    //  re-applied on every start so rotating the password is just a restart.)
    if let (Ok(email), Ok(pass)) = (
        std::env::var("CONSOLE_ADMIN_EMAIL"),
        std::env::var("CONSOLE_ADMIN_PASSWORD"),
    ) {
        let email = email.trim().to_lowercase();
        if !email.is_empty() && pass.len() >= 12 {
            match filehub_backend::auth::hash_password(&pass) {
                Ok(hash) => {
                    let r = sqlx::query(
                        r#"INSERT INTO users (id, email, display_name, avatar_tone, password_hash, role, status)
                           VALUES ($1, $2, 'FileHub Administrator', 'slate', $3, 'admin', 'active')
                           ON CONFLICT (email) DO UPDATE
                             SET password_hash = EXCLUDED.password_hash,
                                 role = 'admin', status = 'active'"#,
                    )
                    .bind(format!("usr_console_{}", email.replace(|c: char| !c.is_alphanumeric(), "_")))
                    .bind(&email)
                    .bind(&hash)
                    .execute(&state.db)
                    .await;
                    match r {
                        Ok(_) => tracing::info!(email = %email, "console admin account provisioned"),
                        Err(e) => tracing::error!("failed to provision console admin account: {e}"),
                    }
                }
                Err(e) => tracing::error!("failed to hash console admin password: {e:?}"),
            }
        } else {
            tracing::warn!("skipping console admin provisioning — email is empty or password is shorter than 12 characters");
        }
    }

    // The method-override layer must be the OUTERMOST layer, applied before the
    // Router matches routes, otherwise the request has already been answered 405.
    // (Some edge proxies only let GET/POST through — see method_override in lib.rs.)
    let app = tower::ServiceBuilder::new()
        .layer(axum::middleware::from_fn(method_override))
        .service(build_router(state));

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8090);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("File Hub backend listening on http://0.0.0.0:{port}");
    axum::serve(
        listener,
        axum::ServiceExt::<axum::extract::Request>::into_make_service(app),
    )
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

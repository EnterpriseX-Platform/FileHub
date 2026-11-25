use anyhow::Context;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::storage::Storage;

pub struct AppState {
    pub db: PgPool,
    pub storage: Storage,
}

impl AppState {
    pub async fn init() -> anyhow::Result<Self> {
        // Fail-fast on missing security-critical env in release builds.  In
        // debug we keep the previous fallback behaviour so `cargo test` and
        // local checkouts stay frictionless.
        check_required_env_release()?;

        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://filehub:filehub@localhost:5434/filehub".into());

        // Pool sized for a moderately concurrent deployment.  The previous
        // value (8) is enough for `cargo test` but starves under real load
        // once the rotation worker, P1 thumbnail jobs, and TUS finalises
        // overlap.  All four knobs are env-tunable per deployment.
        let max_conns: u32 = std::env::var("DATABASE_MAX_CONNECTIONS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(32);
        // Short: when the database is down, requests fail fast instead of piling up.
        let acquire_secs = crate::config::env_num("DATABASE_ACQUIRE_TIMEOUT_SECS", 10);
        let idle_secs: u64 = std::env::var("DATABASE_IDLE_TIMEOUT_SECS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(600);
        let db = PgPoolOptions::new()
            .max_connections(max_conns)
            .acquire_timeout(std::time::Duration::from_secs(acquire_secs))
            .idle_timeout(Some(std::time::Duration::from_secs(idle_secs)))
            // Ping a pooled connection before use (default on): after a load spike
            // or network blip, half-dead connections are dropped instead of
            // failing request after request. `DATABASE_PING_BEFORE_USE=off` saves
            // one round trip per statement on a stable network.
            .test_before_acquire(crate::config::env_flag_default_on("DATABASE_PING_BEFORE_USE"))
            .connect_lazy(&db_url)
            .with_context(|| format!("invalid DATABASE_URL {db_url}"))?;
        // Wait for the database instead of exiting (it may start after us, e.g. a
        // fresh cluster) — up to DATABASE_CONNECT_WAIT_SECS (default 120).
        let wait = std::time::Duration::from_secs(crate::config::env_num("DATABASE_CONNECT_WAIT_SECS", 120));
        let started = std::time::Instant::now();
        loop {
            match sqlx::query("SELECT 1").execute(&db).await {
                Ok(_) => break,
                Err(e) if started.elapsed() < wait => {
                    tracing::warn!("database not reachable yet ({e}); retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(e) => return Err(anyhow::anyhow!(e)).with_context(|| "database not reachable"),
            }
        }

        sqlx::migrate!("./migrations").run(&db).await?;

        // Seed accounts run *after* migrations so the users table exists, and
        // are idempotent — they only insert when the table is empty so a
        // restart never duplicates them.
        // Demo accounts (known passwords) + demo collaboration rows only in
        // DEMO_MODE — a production install must not get an admin account
        // whose password is printed in the public README.
        if crate::seed_demo::demo_mode() {
            crate::auth::bootstrap_seed_users(&db).await?;
            // Best-effort: a failure must never block startup.
            if let Err(e) = crate::seed_demo::bootstrap_demo_data(&db).await {
                tracing::warn!("demo seed skipped: {e:#}");
            }
        } else if std::env::var("PURGE_DEMO_DATA").map(|v| v == "1").unwrap_or(false) {
            if let Err(e) = crate::seed_demo::purge_demo_data(&db).await {
                tracing::warn!("demo purge failed: {e:#}");
            }
        }

        let storage_root = std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./storage".into());
        let storage = Storage::init(&storage_root).await?;

        Ok(Self { db, storage })
    }
}

/// Refuse to boot a release binary without the security-critical env vars set.
/// In debug builds, log warnings instead so dev workflows stay frictionless.
fn check_required_env_release() -> anyhow::Result<()> {
    let required: &[(&str, &str)] = &[
        ("WOPI_SECRET",  "HMAC key for Collabora access tokens — anyone with this key can forge edit tokens for every file"),
        ("CORS_ORIGIN",  "exact browser-facing origin that may call this API"),
        ("DATABASE_URL", "Postgres connection string"),
    ];
    let mut missing: Vec<&str> = Vec::new();
    for (name, _why) in required {
        match std::env::var(name) {
            Ok(v) if !v.is_empty() => {}
            _ => missing.push(name),
        }
    }
    if !missing.is_empty() {
        #[cfg(debug_assertions)] {
            tracing::warn!(
                "[dev] missing env: {:?} — release builds will refuse to boot. \
                 Set these in backend/.env before deploying.",
                missing,
            );
        }
        #[cfg(not(debug_assertions))] {
            anyhow::bail!(
                "refusing to boot in release without required env: {:?}. \
                 See backend/.env.example for descriptions.",
                missing,
            );
        }
    }

    // Loud warning when STORAGE_ENC_KEY is unset — files land in plaintext.
    if std::env::var("STORAGE_ENC_KEY").ok().map(|s| s.is_empty()).unwrap_or(true) {
        tracing::warn!(
            "STORAGE_ENC_KEY not set — uploads will be written to disk in plaintext. \
             Set a 32-byte hex value to enable AES-256-GCM at rest."
        );
    }

    // In a release build, warn if session cookies won't be marked Secure — they
    // would then be sent over plain http and could be sniffed.  COOKIE_SECURE
    // defaults to true; only disable it behind a trusted TLS-terminating proxy.
    #[cfg(not(debug_assertions))]
    if std::env::var("COOKIE_SECURE").ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
        .unwrap_or(false)
    {
        tracing::warn!(
            "COOKIE_SECURE is disabled in a release build — session cookies won't be marked Secure. \
             Only do this behind a trusted TLS-terminating proxy on a private network."
        );
    }
    Ok(())
}

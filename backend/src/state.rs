use anyhow::Context;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::ai::{AiClient, AiConfig};
use crate::storage::Storage;

pub struct AppState {
    pub db: PgPool,
    pub storage: Storage,
    pub ai: AiClient,
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
        let acquire_secs: u64 = std::env::var("DATABASE_ACQUIRE_TIMEOUT_SECS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(30);
        let idle_secs: u64 = std::env::var("DATABASE_IDLE_TIMEOUT_SECS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(600);
        let db = PgPoolOptions::new()
            .max_connections(max_conns)
            .acquire_timeout(std::time::Duration::from_secs(acquire_secs))
            .idle_timeout(Some(std::time::Duration::from_secs(idle_secs)))
            .test_before_acquire(true)
            .connect(&db_url)
            .await
            .with_context(|| format!("connecting to {db_url}"))?;

        sqlx::migrate!("./migrations").run(&db).await?;

        // Seed accounts run *after* migrations so the users table exists, and
        // are idempotent — they only insert when the table is empty so a
        // restart never duplicates them.
        crate::auth::bootstrap_seed_users(&db).await?;

        // Default request forms + their approval templates (the Form Designer's
        // seed data). Non-destructive (ON CONFLICT DO NOTHING) so admin edits
        // survive restarts. Needs the seed reviewer users above.
        if let Err(e) = crate::requests::bootstrap_request_forms(&db).await {
            tracing::warn!("request-forms seed skipped: {e:#}");
        }

        // Demo collaboration data (comment threads, review workflows,
        // notifications, version history). Best-effort: a failure must never
        // block startup, so we log and continue rather than propagate.
        if let Err(e) = crate::seed_demo::bootstrap_demo_data(&db).await {
            tracing::warn!("demo seed skipped: {e:#}");
        }

        let storage_root = std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./storage".into());
        let storage = Storage::init(&storage_root).await?;

        // AI provider — local-first (Ollama) by default; see crate::ai. Disabled
        // cleanly via AI_ENABLED=false, in which case enrichment is skipped and
        // FileHub behaves exactly as before.
        let ai = AiClient::new(AiConfig::from_env());
        tracing::info!(
            "AI provider: enabled={} embed_model={} chat_model={}",
            ai.enabled(), ai.embed_model(), ai.chat_model(),
        );

        Ok(Self { db, storage, ai })
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

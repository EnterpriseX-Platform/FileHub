//! Pluggable outbound email (SMTP). Optional and off by default — when
//! `MAIL_ENABLED` is false or SMTP isn't configured, `send` is a no-op, so
//! local/dev and air-gapped installs behave exactly as before. Used to mirror
//! in-app notifications to email (TOR Annex A: แจ้งเตือนผ่านอีเมล — workflow +
//! e-signature sign-turn alerts).
//!
//! Config (env): MAIL_ENABLED, SMTP_HOST, SMTP_PORT (default 587),
//! SMTP_USER, SMTP_PASS, MAIL_FROM, MAIL_SECURITY (starttls | tls | none),
//! MAIL_BASE_URL (prefix for the links in the body).

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// Email is on only when explicitly enabled *and* a host is configured.
pub fn enabled() -> bool {
    let on = !matches!(
        std::env::var("MAIL_ENABLED").ok().as_deref(),
        None | Some("0") | Some("false") | Some("no") | Some("off")
    );
    on && var("SMTP_HOST").is_some()
}

/// Absolute link prefix for URLs embedded in mail bodies (falls back to a
/// relative path when unset).
pub fn base_url() -> String {
    var("MAIL_BASE_URL").unwrap_or_default()
}

fn transport() -> anyhow::Result<AsyncSmtpTransport<Tokio1Executor>> {
    let host = var("SMTP_HOST").ok_or_else(|| anyhow::anyhow!("SMTP_HOST unset"))?;
    let port: u16 = var("SMTP_PORT").and_then(|v| v.parse().ok()).unwrap_or(587);
    let security = var("MAIL_SECURITY").unwrap_or_else(|| "starttls".into());

    let mut builder = match security.as_str() {
        "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&host),
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&host)?,
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)?, // starttls (default)
    }
    .port(port);

    if let (Some(u), Some(p)) = (var("SMTP_USER"), var("SMTP_PASS")) {
        builder = builder.credentials(Credentials::new(u, p));
    }
    Ok(builder.build())
}

/// Send a plain-text email. Best-effort: returns Ok and does nothing when email
/// is disabled, and never panics. Errors are returned for the caller to log.
pub async fn send(to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let from = var("MAIL_FROM").unwrap_or_else(|| "filehub@localhost".into());
    let email = Message::builder()
        .from(from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())?;
    transport()?.send(email).await?;
    Ok(())
}

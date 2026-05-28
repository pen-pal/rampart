//! System-wide SMTP — used by status-page subscriber fan-out.
//!
//! Stored in the `settings` table under key "smtp" as a JSON blob. The
//! per-monitor email notification channel (`rampart_notifier::channels::
//! email`) has its own copy of nearly identical config; we don't share
//! because status-page emails and monitor-alert emails legitimately
//! want different From addresses in many deployments.

use lettre::message::{header, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use rampart_db::DbPool;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// "tls" (465), "starttls" (587), or "plain" (25 — bring your own
    /// network, don't expose this to the internet).
    pub encryption: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// Mailbox the email is sent from. Format: `Name <addr@host>` or
    /// just `addr@host`.
    pub from: String,
}

pub async fn load(pool: &DbPool) -> Result<Option<SmtpConfig>, String> {
    let raw = rampart_db::settings::get(pool, "smtp")
        .await
        .map_err(|e| e.to_string())?;
    match raw {
        None => Ok(None),
        Some(v) => serde_json::from_value::<SmtpConfig>(v)
            .map(Some)
            .map_err(|e| format!("smtp config: {e}")),
    }
}

pub async fn send(
    cfg: &SmtpConfig,
    to: &str,
    subject: &str,
    body_text: &str,
) -> Result<(), String> {
    let from = Mailbox::from_str(&cfg.from).map_err(|e| format!("from addr: {e}"))?;
    let to_mb = Mailbox::from_str(to).map_err(|e| format!("to addr: {e}"))?;

    let msg = Message::builder()
        .from(from)
        .to(to_mb)
        .subject(subject)
        .header(header::ContentType::TEXT_PLAIN)
        .body(body_text.to_string())
        .map_err(|e| format!("build: {e}"))?;

    let builder = match cfg.encryption.as_str() {
        "tls" => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host).map_err(|e| e.to_string())?
        }
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)
            .map_err(|e| e.to_string())?,
        "plain" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host),
        other => return Err(format!("unknown encryption: {other}")),
    };
    let mut builder = builder.port(cfg.port);
    if let (Some(u), Some(p)) = (cfg.username.as_deref(), cfg.password.as_deref()) {
        builder = builder.credentials(Credentials::new(u.into(), p.into()));
    }
    let transport = builder.build();

    transport
        .send(msg)
        .await
        .map_err(|e| format!("send: {e}"))?;
    Ok(())
}

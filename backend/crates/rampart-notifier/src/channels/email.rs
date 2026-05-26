//! Email via SMTP (lettre + rustls).
//!
//! Configure with an SMTP host/port + credentials and a from address.
//! Optional `to` overrides per channel; otherwise we deliver to `default_to`.
//!
//! For Gmail use `smtp.gmail.com:587` (STARTTLS) with an app password.
//! For SendGrid: `smtp.sendgrid.net:587` with API key as password.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use lettre::{
    message::{Mailbox, MultiPart, SinglePart, header::ContentType},
    transport::smtp::{authentication::Credentials, AsyncSmtpTransport, AsyncSmtpTransportBuilder},
    AsyncTransport, Message, Tokio1Executor,
};
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
pub struct EmailConfig {
    pub smtp_host: String,
    #[serde(default = "default_port")]
    pub smtp_port: u16,
    /// "starttls" (default, port 587), "tls" (implicit TLS, port 465), "plain" (no encryption).
    #[serde(default = "default_encryption")]
    pub encryption: String,
    #[serde(default)]
    pub smtp_user: Option<String>,
    #[serde(default)]
    pub smtp_password: Option<String>,
    pub from: String,
    /// Comma-separated list of default recipients.
    pub to: String,
    #[serde(default)]
    pub reply_to: Option<String>,
}
fn default_port() -> u16 { 587 }
fn default_encryption() -> String { "starttls".into() }

pub struct Email {
    cfg: EmailConfig,
}

impl Email {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: EmailConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.smtp_host.is_empty() || cfg.from.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("smtp_host, from, and to are required".into()));
        }
        Ok(Self { cfg })
    }

    fn build_transport(&self) -> Result<AsyncSmtpTransport<Tokio1Executor>, ChannelError> {
        let builder: AsyncSmtpTransportBuilder = match self.cfg.encryption.as_str() {
            "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&self.cfg.smtp_host)
                .map_err(|e| ChannelError::BadConfig(format!("smtp relay: {e}")))?,
            "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.cfg.smtp_host)
                .map_err(|e| ChannelError::BadConfig(format!("smtp starttls relay: {e}")))?,
            "plain" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.cfg.smtp_host),
            other => return Err(ChannelError::BadConfig(format!("unknown encryption: {other}"))),
        };
        let mut builder = builder.port(self.cfg.smtp_port);
        if let (Some(u), Some(p)) = (&self.cfg.smtp_user, &self.cfg.smtp_password) {
            builder = builder.credentials(Credentials::new(u.clone(), p.clone()));
        }
        Ok(builder.build())
    }
}

#[async_trait]
impl Channel for Email {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let from: Mailbox = Mailbox::from_str(&self.cfg.from)
            .map_err(|e| ChannelError::BadConfig(format!("from address: {e}")))?;
        let mut builder = Message::builder().from(from.clone()).subject(subject);

        for addr in self.cfg.to.split(',') {
            let addr = addr.trim();
            if addr.is_empty() { continue; }
            let mb = Mailbox::from_str(addr)
                .map_err(|e| ChannelError::BadConfig(format!("to address {addr:?}: {e}")))?;
            builder = builder.to(mb);
        }
        if let Some(reply) = &self.cfg.reply_to {
            let mb = Mailbox::from_str(reply)
                .map_err(|e| ChannelError::BadConfig(format!("reply_to: {e}")))?;
            builder = builder.reply_to(mb);
        }

        // Send both plain text and a minimal HTML version. Most clients
        // prefer the HTML; mail relays without HTML support fall back to
        // the text/plain part.
        let html_body = format!(
            "<p style=\"font-family:Inter,system-ui,sans-serif\">{}</p><pre style=\"font-family:JetBrains Mono,monospace;background:#f5f5f4;padding:12px;border-radius:8px;font-size:12px\">{}</pre>",
            html_escape(subject),
            html_escape(body),
        );
        let email = builder
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(body.to_string()))
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(html_body)),
            )
            .map_err(|e| ChannelError::BadConfig(format!("message build: {e}")))?;

        let mailer = self.build_transport()?;
        mailer.send(email).await
            .map_err(|e| ChannelError::Other(format!("smtp send: {e}")))?;
        Ok(())
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

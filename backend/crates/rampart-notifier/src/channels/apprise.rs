//! Apprise gateway — single adapter that proxies to 80+ upstream services
//! via a user-deployed apprise-api server (https://github.com/caronc/apprise-api).
//!
//! Setup:
//!   1. Run apprise-api as a sidecar:
//!        docker run -d --name apprise -p 8000:8000 caronc/apprise:latest
//!   2. Configure Rampart with `apprise_url: "http://apprise:8000"` (or
//!      whatever hostname you reach it on).
//!   3. Add one or more apprise:// URLs:
//!        tgram://botid:token/chatid
//!        discord://webhook_id/webhook_token
//!        pbul://accesstoken
//!        mailto://user:pass@gmail.com
//!        signal://account/recipient
//!        mqtt://user:pass@broker.local/topic
//!        ... and ~80 more.
//!
//! See https://github.com/caronc/apprise/wiki for the full URL syntax.
//!
//! One Rampart "Apprise" channel can fan out to many upstream services
//! at once (comma-separated list of URLs).

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AppriseConfig {
    /// Base URL of the apprise-api server.
    pub apprise_url: String,
    /// One or more apprise:// URLs. Comma-separated; whitespace stripped.
    pub urls:        String,
    /// Optional explicit notification type. Otherwise we map from status:
    /// up=success, down=failure, warn=warning, other=info.
    #[serde(default)]
    pub notify_type: Option<String>,
}

pub struct Apprise {
    cfg:    AppriseConfig,
    client: reqwest::Client,
}

impl Apprise {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AppriseConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.apprise_url.starts_with("http://") && !cfg.apprise_url.starts_with("https://") {
            return Err(ChannelError::BadConfig("apprise_url must start with http(s)://".into()));
        }
        if cfg.urls.trim().is_empty() {
            return Err(ChannelError::BadConfig(
                "at least one apprise:// URL is required (e.g. tgram://, discord://, pbul://)".into(),
            ));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct AppriseRequest<'a> {
    title: &'a str,
    body:  &'a str,
    urls:  String,
    #[serde(rename = "type")]
    notify_type: &'a str,
    format:      &'a str,
}

#[async_trait]
impl Channel for Apprise {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let notify_type = self.cfg.notify_type.as_deref().unwrap_or(match event.heartbeat.status {
            rampart_core::MonitorStatus::Up   => "success",
            rampart_core::MonitorStatus::Down => "failure",
            rampart_core::MonitorStatus::Warn => "warning",
            _                                 => "info",
        });

        // Normalize: strip whitespace around the comma-separated URLs so a
        // user pasting one-per-line value (with spaces) still works.
        let urls = self.cfg.urls
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");

        let payload = AppriseRequest {
            title: subject,
            body,
            urls,
            notify_type,
            format: "text",
        };

        let endpoint = format!("{}/notify", self.cfg.apprise_url.trim_end_matches('/'));
        let resp = self.client.post(&endpoint).json(&payload).send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}

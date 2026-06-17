//! ntfy.sh — simple HTTP-based push notifications.
//!
//! Setup: pick a topic name (e.g. `rampart-alerts-myhomelab-x9z`),
//! subscribe to it in the ntfy mobile/desktop app, then publish here
//! against the same topic. Public ntfy.sh works without auth; self-host
//! supports basic auth + Bearer tokens.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct NtfyConfig {
    #[serde(default = "default_server")]
    pub server: String,
    pub topic: String,
    /// Priority 1..5 (1=min, 3=default, 5=urgent).
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Optional Bearer token (self-hosted ntfy with auth) or basic auth.
    #[serde(default)]
    pub auth: Option<String>,
    /// Optional explicit tags. Common ones: rotating_light, white_check_mark, warning.
    #[serde(default)]
    pub tags: Vec<String>,
}
fn default_server() -> String {
    "https://ntfy.sh".into()
}
fn default_priority() -> u8 {
    3
}

#[derive(Debug)]
pub struct Ntfy {
    cfg: NtfyConfig,
    client: reqwest::Client,
}

impl Ntfy {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: NtfyConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.topic.is_empty() {
            return Err(ChannelError::BadConfig("topic is required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[async_trait]
impl Channel for Ntfy {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        // ntfy uses header-based metadata. Body is the message text.
        let url = format!(
            "{}/{}",
            self.cfg.server.trim_end_matches('/'),
            self.cfg.topic
        );
        let mut tags = self.cfg.tags.clone();
        if tags.is_empty() {
            tags.push(
                match event.heartbeat.status {
                    rampart_core::MonitorStatus::Up => "white_check_mark",
                    rampart_core::MonitorStatus::Down => "rotating_light",
                    _ => "warning",
                }
                .to_string(),
            );
        }

        let mut req = self
            .client
            .post(&url)
            .header("Title", header_safe(subject))
            .header("Priority", self.cfg.priority.to_string())
            .header("Tags", tags.join(","))
            .body(body.to_string());
        if let Some(a) = &self.cfg.auth {
            req = req.header("Authorization", a);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}

// ntfy headers reject CR/LF; strip them defensively.
fn header_safe(s: &str) -> String {
    s.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requires_topic() {
        assert!(Ntfy::from_config(&json!({})).is_err());
    }

    #[test]
    fn defaults_server_to_ntfy_sh_and_priority_3() {
        let raw = json!({"topic": "my-topic"});
        let cfg: NtfyConfig = serde_json::from_value(raw).unwrap();
        assert_eq!(cfg.server, "https://ntfy.sh");
        assert_eq!(cfg.priority, 3);
    }

    #[test]
    fn header_safe_strips_cr_lf() {
        assert_eq!(header_safe("line1\nline2\rend"), "line1line2end");
    }
}

//! Slack incoming webhook.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SlackConfig {
    pub webhook_url: String,
    /// Optional explicit channel override (e.g. "#alerts"). Slack webhooks
    /// usually pin a channel server-side; this is only useful for legacy
    /// "incoming webhook" integrations that allow channel overrides.
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(Debug)]
pub struct Slack {
    cfg: SlackConfig,
    client: reqwest::Client,
}

impl Slack {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SlackConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://hooks.slack.com/") {
            return Err(ChannelError::BadConfig(
                "webhook_url must start with https://hooks.slack.com/".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct SlackPayload<'a> {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: &'a Option<String>,
    attachments: Vec<SlackAttachment>,
}

#[derive(Serialize)]
struct SlackAttachment {
    color: &'static str,
    title: String,
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_non_slack_webhook_host() {
        let err =
            Slack::from_config(&json!({"webhook_url": "https://example.com/hooks/x"})).unwrap_err();
        assert!(matches!(err, ChannelError::BadConfig(_)));
    }

    #[test]
    fn accepts_valid_slack_webhook() {
        let ok =
            Slack::from_config(&json!({"webhook_url": "https://hooks.slack.com/services/T/B/x"}));
        assert!(ok.is_ok());
    }

    #[test]
    fn rejects_missing_webhook_url() {
        let err = Slack::from_config(&json!({})).unwrap_err();
        assert!(matches!(err, ChannelError::BadConfig(_)));
    }
}

#[async_trait]
impl Channel for Slack {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        // Color the attachment by status — Slack convention is green/red/yellow.
        let color = match event.heartbeat.status {
            rampart_core::MonitorStatus::Up => "good",
            rampart_core::MonitorStatus::Down => "danger",
            _ => "warning",
        };
        let payload = SlackPayload {
            text: subject.to_string(),
            channel: &self.cfg.channel,
            attachments: vec![SlackAttachment {
                color,
                title: subject.to_string(),
                text: body.to_string(),
            }],
        };
        let resp = self
            .client
            .post(&self.cfg.webhook_url)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}

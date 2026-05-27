//! Discord incoming webhook.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct DiscordConfig {
    pub webhook_url: String,
    #[serde(default)]
    pub username:    Option<String>,
}

#[derive(Debug)]
pub struct Discord {
    cfg:    DiscordConfig,
    client: reqwest::Client,
}

impl Discord {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: DiscordConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.webhook_url.starts_with("https://discord.com/api/webhooks/") &&
           !cfg.webhook_url.starts_with("https://discordapp.com/api/webhooks/") {
            return Err(ChannelError::BadConfig(
                "webhook_url must be a discord.com webhook URL".into(),
            ));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct DiscordPayload<'a> {
    content:  String,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: &'a Option<String>,
    embeds:   Vec<DiscordEmbed>,
}

#[derive(Serialize)]
struct DiscordEmbed {
    title:       String,
    description: String,
    color:       u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_non_discord_webhook_host() {
        let err = Discord::from_config(&json!({"webhook_url": "https://example.com/hook"})).unwrap_err();
        assert!(matches!(err, ChannelError::BadConfig(_)));
    }

    #[test]
    fn accepts_both_discord_and_discordapp_hosts() {
        assert!(Discord::from_config(&json!({"webhook_url": "https://discord.com/api/webhooks/1/x"})).is_ok());
        assert!(Discord::from_config(&json!({"webhook_url": "https://discordapp.com/api/webhooks/1/x"})).is_ok());
    }
}

#[async_trait]
impl Channel for Discord {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        // Discord embeds use a decimal RGB int. Green / red / amber.
        let color = match event.heartbeat.status {
            rampart_core::MonitorStatus::Up   => 0x10b981,
            rampart_core::MonitorStatus::Down => 0xef4444,
            _                                 => 0xf59e0b,
        };
        let payload = DiscordPayload {
            content:  subject.to_string(),
            username: &self.cfg.username,
            embeds:   vec![DiscordEmbed {
                title:       subject.to_string(),
                description: body.to_string(),
                color,
            }],
        };
        let resp = self.client.post(&self.cfg.webhook_url).json(&payload).send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}

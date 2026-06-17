//! Mastodon — post a status (toot) via /api/v1/statuses.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MastodonConfig {
    pub server: String,
    pub access_token: String,
    /// "public" | "unlisted" | "private" | "direct"; default "private".
    #[serde(default = "default_visibility")]
    pub visibility: String,
}
fn default_visibility() -> String {
    "private".into()
}

pub struct Mastodon {
    cfg: MastodonConfig,
    client: reqwest::Client,
}

impl Mastodon {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MastodonConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_token.trim().is_empty() {
            return Err(ChannelError::BadConfig("access_token required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    status: String,
    visibility: &'a str,
}

#[async_trait]
impl Channel for Mastodon {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        // 500 char default cap (Mastodon's). Truncate hard rather than 422.
        let mut status = format!("{subject}\n\n{body}");
        if status.chars().count() > 500 {
            status = status.chars().take(497).collect::<String>() + "…";
        }
        let url = format!("{}/api/v1/statuses", self.cfg.server.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.cfg.access_token)
            .json(&Payload {
                status,
                visibility: &self.cfg.visibility,
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}

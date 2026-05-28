//! Zulip — send a message via the REST API. Auth: bot email + API key
//! over HTTP basic. Posts to a stream + topic (or to a private user).

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ZulipConfig {
    pub server: String,
    pub bot_email: String,
    pub bot_key: String,
    /// "stream" or "private"; default "stream".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// For stream: the stream name. For private: a comma-separated list
    /// of user emails or a single email.
    pub to: String,
    /// Only used for kind = "stream".
    #[serde(default)]
    pub topic: Option<String>,
}
fn default_kind() -> String {
    "stream".into()
}

pub struct Zulip {
    cfg: ZulipConfig,
    client: reqwest::Client,
}

impl Zulip {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: ZulipConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.server.trim().is_empty()
            || cfg.bot_email.trim().is_empty()
            || cfg.bot_key.trim().is_empty()
        {
            return Err(ChannelError::BadConfig(
                "server + bot_email + bot_key required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Zulip {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("{}/api/v1/messages", self.cfg.server.trim_end_matches('/'));
        let content = format!("**{subject}**\n\n{body}");
        let mut form = vec![
            ("type", self.cfg.kind.clone()),
            ("to", self.cfg.to.clone()),
            ("content", content),
        ];
        if self.cfg.kind == "stream" {
            form.push((
                "topic",
                self.cfg.topic.clone().unwrap_or_else(|| "rampart".into()),
            ));
        }
        let resp = self
            .client
            .post(&url)
            .basic_auth(&self.cfg.bot_email, Some(&self.cfg.bot_key))
            .form(&form)
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

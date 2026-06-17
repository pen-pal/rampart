//! Trello — POST /1/cards?key=<key>&token=<token>&idList=<list>&name=<>&desc=<>

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TrelloConfig {
    pub key: String,
    pub token: String,
    pub list_id: String,
}

pub struct Trello {
    cfg: TrelloConfig,
    client: reqwest::Client,
}

impl Trello {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: TrelloConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.key.is_empty() || cfg.token.is_empty() || cfg.list_id.is_empty() {
            return Err(ChannelError::BadConfig(
                "key + token + list_id required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[async_trait]
impl Channel for Trello {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let resp = self
            .client
            .post("https://api.trello.com/1/cards")
            .query(&[
                ("key", self.cfg.key.as_str()),
                ("token", self.cfg.token.as_str()),
                ("idList", self.cfg.list_id.as_str()),
                ("name", subject),
                ("desc", body),
            ])
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

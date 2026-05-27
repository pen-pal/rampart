//! Notion — POST /v1/pages with bearer integration token. Creates a
//! new page in a database keyed by `Name` (title) + `Status` text.
//! Requires the database's title property to be named "Name" — this is
//! the default for new Notion databases.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct NotionConfig {
    pub api_token:   String,
    pub database_id: String,
}

pub struct Notion { cfg: NotionConfig, client: reqwest::Client }

impl Notion {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: NotionConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_token.is_empty() || cfg.database_id.is_empty() {
            return Err(ChannelError::BadConfig("api_token + database_id required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[async_trait]
impl Channel for Notion {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = json!({
            "parent": { "database_id": self.cfg.database_id },
            "properties": {
                "Name": {
                    "title": [{ "text": { "content": subject } }]
                },
            },
            "children": [{
                "object": "block",
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{ "type": "text", "text": { "content": body } }]
                }
            }]
        });
        let resp = self.client.post("https://api.notion.com/v1/pages")
            .bearer_auth(&self.cfg.api_token)
            .header("Notion-Version", "2022-06-28")
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}

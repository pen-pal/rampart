//! ServerChan (Server酱) — sct.ftqq.com push to WeChat via SendKey.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ServerchanConfig {
    pub send_key: String,
}

pub struct Serverchan {
    cfg: ServerchanConfig,
    client: reqwest::Client,
}

impl Serverchan {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: ServerchanConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.send_key.trim().is_empty() {
            return Err(ChannelError::BadConfig("send_key required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[async_trait]
impl Channel for Serverchan {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("https://sctapi.ftqq.com/{}.send", self.cfg.send_key);
        let resp = self
            .client
            .post(&url)
            .form(&[("title", subject), ("desp", body)])
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

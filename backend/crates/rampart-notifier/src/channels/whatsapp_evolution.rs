//! WhatsApp via Evolution API — POST /message/sendText/<instance>.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct EvolutionConfig {
    pub base_url: String,
    pub api_key: String,
    pub instance: String,
    /// E.164 number without '+'
    pub number: String,
}

pub struct WhatsappEvolution {
    cfg: EvolutionConfig,
    client: reqwest::Client,
}

impl WhatsappEvolution {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: EvolutionConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.instance.is_empty() || cfg.number.is_empty() {
            return Err(ChannelError::BadConfig(
                "api_key + instance + number required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    number: &'a str,
    text: String,
}

#[async_trait]
impl Channel for WhatsappEvolution {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/message/sendText/{}",
            self.cfg.base_url.trim_end_matches('/'),
            self.cfg.instance,
        );
        let payload = Payload {
            number: &self.cfg.number,
            text: format!("{subject}\n{body}"),
        };
        let resp = self
            .client
            .post(&url)
            .header("apikey", &self.cfg.api_key)
            .json(&payload)
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

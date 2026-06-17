//! Alerta — POST /api/alert with an API key.
//! https://docs.alerta.io/api/reference.html#alert

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AlertaConfig {
    pub api_url: String,
    pub api_key: String,
    #[serde(default = "default_env")]
    pub environment: String,
}
fn default_env() -> String {
    "Production".into()
}

pub struct Alerta {
    cfg: AlertaConfig,
    client: reqwest::Client,
}

impl Alerta {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AlertaConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.trim().is_empty() {
            return Err(ChannelError::BadConfig("api_key required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    resource: &'a str,
    event: &'a str,
    environment: &'a str,
    severity: &'a str,
    service: Vec<&'a str>,
    text: &'a str,
    origin: &'static str,
}

#[async_trait]
impl Channel for Alerta {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let severity = match event.heartbeat.status {
            MonitorStatus::Up => "ok",
            MonitorStatus::Warn => "warning",
            MonitorStatus::Down => "critical",
            _ => "informational",
        };
        let payload = Payload {
            resource: &event.monitor.name,
            event: subject,
            environment: &self.cfg.environment,
            severity,
            service: vec!["rampart"],
            text: body,
            origin: "rampart",
        };
        let url = format!("{}/alert", self.cfg.api_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Key {}", self.cfg.api_key))
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

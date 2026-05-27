//! Rollbar — POST /api/1/item/ with access_token + message body.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RollbarConfig {
    pub access_token: String,
    #[serde(default = "default_env")]
    pub environment:  String,
}
fn default_env() -> String { "production".into() }

pub struct Rollbar { cfg: RollbarConfig, client: reqwest::Client }

impl Rollbar {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: RollbarConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_token.is_empty() {
            return Err(ChannelError::BadConfig("access_token required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    access_token: &'a str,
    data:         Data<'a>,
}
#[derive(Serialize)]
struct Data<'a> {
    environment: &'a str,
    level:       &'a str,
    body:        Body<'a>,
}
#[derive(Serialize)]
struct Body<'a> { message: Msg<'a> }
#[derive(Serialize)]
struct Msg<'a> { body: String, monitor: &'a str }

#[async_trait]
impl Channel for Rollbar {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let level = match event.heartbeat.status {
            MonitorStatus::Up   => "info",
            MonitorStatus::Warn => "warning",
            _                    => "error",
        };
        let payload = Payload {
            access_token: &self.cfg.access_token,
            data: Data {
                environment: &self.cfg.environment,
                level,
                body: Body { message: Msg { body: format!("{subject}\n{body}"), monitor: &event.monitor.name } },
            },
        };
        let resp = self.client.post("https://api.rollbar.com/api/1/item/")
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}

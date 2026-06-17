//! Honeybadger — POST /v1/notices with X-API-Key.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct HoneybadgerConfig {
    pub api_key: String,
    #[serde(default = "default_env")]
    pub environment: String,
}
fn default_env() -> String {
    "production".into()
}

pub struct Honeybadger {
    cfg: HoneybadgerConfig,
    client: reqwest::Client,
}

impl Honeybadger {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: HoneybadgerConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() {
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
    notifier: Notifier<'a>,
    error: HbError<'a>,
    server: Server<'a>,
}
#[derive(Serialize)]
struct Notifier<'a> {
    name: &'static str,
    version: &'static str,
    url: &'a str,
}
#[derive(Serialize)]
struct HbError<'a> {
    class: &'static str,
    message: String,
    tags: Vec<&'a str>,
}
#[derive(Serialize)]
struct Server<'a> {
    environment_name: &'a str,
}

#[async_trait]
impl Channel for Honeybadger {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            notifier: Notifier {
                name: "rampart",
                version: env!("CARGO_PKG_VERSION"),
                url: "https://github.com/pen-pal/rampart",
            },
            error: HbError {
                class: "MonitorAlert",
                message: format!("{subject}\n{body}"),
                tags: vec![&event.monitor.name],
            },
            server: Server {
                environment_name: &self.cfg.environment,
            },
        };
        let resp = self
            .client
            .post("https://api.honeybadger.io/v1/notices")
            .header("X-API-Key", &self.cfg.api_key)
            .header("Accept", "application/json")
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

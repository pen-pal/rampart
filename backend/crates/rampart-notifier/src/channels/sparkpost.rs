//! SparkPost — POST /api/v1/transmissions with Authorization: <api_key>.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SparkpostConfig {
    pub api_key: String,
    pub from: String,
    /// comma-separated emails
    pub to: String,
    #[serde(default = "default_base")]
    pub base_url: String,
}
fn default_base() -> String {
    "https://api.sparkpost.com".into()
}

pub struct Sparkpost {
    cfg: SparkpostConfig,
    client: reqwest::Client,
}

impl Sparkpost {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SparkpostConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.to.is_empty() {
            return Err(ChannelError::BadConfig("api_key + to required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    options: Options,
    content: Content<'a>,
    recipients: Vec<Rcpt<'a>>,
}
#[derive(Serialize)]
struct Options {
    sandbox: bool,
}
#[derive(Serialize)]
struct Content<'a> {
    from: &'a str,
    subject: &'a str,
    text: &'a str,
}
#[derive(Serialize)]
struct Rcpt<'a> {
    address: Addr<'a>,
}
#[derive(Serialize)]
struct Addr<'a> {
    email: &'a str,
}

#[async_trait]
impl Channel for Sparkpost {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            options: Options { sandbox: false },
            content: Content {
                from: &self.cfg.from,
                subject,
                text: body,
            },
            recipients: self
                .cfg
                .to
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|e| Rcpt {
                    address: Addr { email: e },
                })
                .collect(),
        };
        let url = format!(
            "{}/api/v1/transmissions",
            self.cfg.base_url.trim_end_matches('/')
        );
        let resp = self
            .client
            .post(&url)
            .header("Authorization", &self.cfg.api_key)
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

//! Mailjet — POST /v3.1/send with basic auth (api_key + api_secret).

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MailjetConfig {
    pub api_key: String,
    pub api_secret: String,
    pub from_email: String,
    #[serde(default)]
    pub from_name: Option<String>,
    pub to_email: String,
    #[serde(default)]
    pub to_name: Option<String>,
}

pub struct Mailjet {
    cfg: MailjetConfig,
    client: reqwest::Client,
}

impl Mailjet {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MailjetConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.api_secret.is_empty() {
            return Err(ChannelError::BadConfig(
                "api_key + api_secret required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    #[serde(rename = "Messages")]
    messages: Vec<Msg<'a>>,
}
#[derive(Serialize)]
struct Msg<'a> {
    #[serde(rename = "From")]
    from: Addr<'a>,
    #[serde(rename = "To")]
    to: Vec<Addr<'a>>,
    #[serde(rename = "Subject")]
    subject: &'a str,
    #[serde(rename = "TextPart")]
    text_part: &'a str,
}
#[derive(Serialize)]
struct Addr<'a> {
    #[serde(rename = "Email")]
    email: &'a str,
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: &'a Option<String>,
}

#[async_trait]
impl Channel for Mailjet {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Payload {
            messages: vec![Msg {
                from: Addr {
                    email: &self.cfg.from_email,
                    name: &self.cfg.from_name,
                },
                to: vec![Addr {
                    email: &self.cfg.to_email,
                    name: &self.cfg.to_name,
                }],
                subject,
                text_part: body,
            }],
        };
        let resp = self
            .client
            .post("https://api.mailjet.com/v3.1/send")
            .basic_auth(&self.cfg.api_key, Some(&self.cfg.api_secret))
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

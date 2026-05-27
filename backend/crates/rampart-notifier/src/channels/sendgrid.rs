//! SendGrid — transactional email via api.sendgrid.com/v3/mail/send.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SendgridConfig {
    pub api_key:    String,
    pub from_email: String,
    #[serde(default)]
    pub from_name:  Option<String>,
    /// Comma-separated list or array. We accept either to be permissive.
    pub to:         Recipients,
}

#[derive(Debug)]
pub struct Recipients(pub Vec<String>);

impl<'de> Deserialize<'de> for Recipients {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if let Some(s) = v.as_str() {
            return Ok(Recipients(s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()));
        }
        if let Some(arr) = v.as_array() {
            return Ok(Recipients(arr.iter().filter_map(|x| x.as_str().map(String::from)).collect()));
        }
        Err(serde::de::Error::custom("to must be a string or array of strings"))
    }
}

pub struct Sendgrid { cfg: SendgridConfig, client: reqwest::Client }

impl Sendgrid {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SendgridConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.trim().is_empty() || cfg.to.0.is_empty() {
            return Err(ChannelError::BadConfig("api_key and to required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    personalizations: Vec<Personalization<'a>>,
    from: From_<'a>,
    subject: &'a str,
    content: Vec<Content<'a>>,
}
#[derive(Serialize)]
struct Personalization<'a> { to: Vec<Addr<'a>> }
#[derive(Serialize)]
struct Addr<'a> { email: &'a str }
#[derive(Serialize)]
struct From_<'a> {
    email: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: &'a Option<String>,
}
#[derive(Serialize)]
struct Content<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    value: &'a str,
}

#[async_trait]
impl Channel for Sendgrid {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let to_list: Vec<Addr<'_>> = self.cfg.to.0.iter().map(|e| Addr { email: e }).collect();
        let payload = Payload {
            personalizations: vec![Personalization { to: to_list }],
            from: From_ { email: &self.cfg.from_email, name: &self.cfg.from_name },
            subject,
            content: vec![Content { kind: "text/plain", value: body }],
        };
        let resp = self.client.post("https://api.sendgrid.com/v3/mail/send")
            .bearer_auth(&self.cfg.api_key)
            .json(&payload)
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}

//! Bark — push to iOS via day.app / self-hosted Bark server.
//!
//! API: `https://<server>/<device_key>/<title>/<body>?...`. Body and
//! title are url-encoded. Optional query params for sound, group, icon.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BarkConfig {
    pub device_key: String,
    #[serde(default = "default_server")]
    pub server: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub sound: Option<String>,
}
fn default_server() -> String {
    "https://api.day.app".into()
}

pub struct Bark {
    cfg: BarkConfig,
    client: reqwest::Client,
}

impl Bark {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: BarkConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.device_key.trim().is_empty() {
            return Err(ChannelError::BadConfig("device_key required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for Bark {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let base = self.cfg.server.trim_end_matches('/');
        let url = format!(
            "{base}/{}/{}/{}",
            self.cfg.device_key,
            urlencode(subject),
            urlencode(body),
        );
        let mut req = self.client.get(&url);
        if let Some(g) = &self.cfg.group {
            req = req.query(&[("group", g)]);
        }
        if let Some(s) = &self.cfg.sound {
            req = req.query(&[("sound", s)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

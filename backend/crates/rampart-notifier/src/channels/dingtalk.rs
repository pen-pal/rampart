//! DingTalk (钉钉) custom robot.
//!
//! Endpoint: https://oapi.dingtalk.com/robot/send?access_token=<token>
//! Optional HMAC signing when the bot is configured with a secret.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
pub struct DingTalkConfig {
    pub access_token: String,
    /// Required only if the bot has signature security enabled.
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub at_mobiles: Vec<String>,
    #[serde(default)]
    pub at_all: bool,
}

pub struct DingTalk {
    cfg: DingTalkConfig,
    client: reqwest::Client,
}

impl DingTalk {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: DingTalkConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_token.trim().is_empty() {
            return Err(ChannelError::BadConfig("access_token required".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }

    fn build_url(&self) -> String {
        let base = format!(
            "https://oapi.dingtalk.com/robot/send?access_token={}",
            self.cfg.access_token,
        );
        let Some(secret) = self.cfg.secret.as_deref() else {
            return base;
        };
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let to_sign = format!("{ts}\n{secret}");
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
        mac.update(to_sign.as_bytes());
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let sig_url = urlencoding(&sig_b64);
        format!("{base}&timestamp={ts}&sign={sig_url}")
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    msgtype: &'static str,
    text: TextBody,
    at: AtSpec<'a>,
}
#[derive(Serialize)]
struct TextBody {
    content: String,
}
#[derive(Serialize)]
struct AtSpec<'a> {
    #[serde(rename = "atMobiles", skip_serializing_if = "Vec::is_empty")]
    at_mobiles: &'a Vec<String>,
    #[serde(rename = "isAtAll")]
    is_at_all: bool,
}

#[async_trait]
impl Channel for DingTalk {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = self.build_url();
        let payload = Payload {
            msgtype: "text",
            text: TextBody {
                content: format!("{subject}\n{body}"),
            },
            at: AtSpec {
                at_mobiles: &self.cfg.at_mobiles,
                is_at_all: self.cfg.at_all,
            },
        };
        let resp = self.client.post(&url).json(&payload).send().await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("errcode").and_then(|c| c.as_i64()) != Some(0) {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

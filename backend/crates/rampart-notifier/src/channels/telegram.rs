//! Telegram via the Bot API.
//!
//! Set up: create a bot with @BotFather → copy the token. Send the bot a
//! message, then visit `https://api.telegram.org/bot<TOKEN>/getUpdates`
//! to find the `chat.id` to deliver to.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id:   String,
    /// "HTML" or "MarkdownV2" — defaults to HTML which is forgiving.
    #[serde(default = "default_parse_mode")]
    pub parse_mode: String,
}
fn default_parse_mode() -> String { "HTML".into() }

pub struct Telegram {
    cfg:    TelegramConfig,
    client: reqwest::Client,
}

impl Telegram {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: TelegramConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.bot_token.is_empty() || cfg.chat_id.is_empty() {
            return Err(ChannelError::BadConfig("bot_token and chat_id are required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct SendPayload<'a> {
    chat_id:    &'a str,
    text:       String,
    parse_mode: &'a str,
}

#[async_trait]
impl Channel for Telegram {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        // Telegram caps a single message at 4096 chars; truncate to be safe.
        let combined = format!("<b>{}</b>\n\n{}", html_escape(subject), html_escape(body));
        let text = if combined.len() > 4000 { combined[..4000].to_string() } else { combined };

        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.cfg.bot_token);
        let payload = SendPayload {
            chat_id:    &self.cfg.chat_id,
            text,
            parse_mode: &self.cfg.parse_mode,
        };
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}

// Minimal HTML escaper for Telegram's HTML parse_mode.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

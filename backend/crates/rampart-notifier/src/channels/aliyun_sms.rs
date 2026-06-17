//! Aliyun SMS — dysmsapi.aliyuncs.com SendSms.
//!
//! Aliyun requires a signed request (HMAC-SHA1 over a normalized
//! parameter string). All requests carry a sign + ts + nonce.
//! template_code + sign_name are pre-registered on the Aliyun console.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
pub struct AliyunSmsConfig {
    pub access_key_id: String,
    pub access_key_secret: String,
    pub sign_name: String,
    pub template_code: String,
    pub phone_numbers: String,
    /// Free-form params injected into the template; sent as JSON.
    #[serde(default)]
    pub template_param: serde_json::Value,
}

pub struct AliyunSms {
    cfg: AliyunSmsConfig,
    client: reqwest::Client,
}

impl AliyunSms {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AliyunSmsConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_key_id.is_empty()
            || cfg.access_key_secret.is_empty()
            || cfg.template_code.is_empty()
            || cfg.phone_numbers.is_empty()
        {
            return Err(ChannelError::BadConfig(
                "aliyun sms missing required fields".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[async_trait]
impl Channel for AliyunSms {
    async fn send(&self, _subject: &str, _body: &str, _event: &Event) -> Result<(), ChannelError> {
        let nonce = format!("{}", rand::random::<u32>());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Aliyun expects ISO 8601 UTC.
        let ts = format_iso8601(now);

        // The TemplateParam field is sent as a JSON string.
        let tpl = if self.cfg.template_param.is_null() {
            "{}".to_string()
        } else {
            self.cfg.template_param.to_string()
        };

        let mut params: Vec<(&str, String)> = vec![
            ("AccessKeyId", self.cfg.access_key_id.clone()),
            ("Action", "SendSms".into()),
            ("Format", "JSON".into()),
            ("PhoneNumbers", self.cfg.phone_numbers.clone()),
            ("RegionId", "cn-hangzhou".into()),
            ("SignName", self.cfg.sign_name.clone()),
            ("SignatureMethod", "HMAC-SHA1".into()),
            ("SignatureNonce", nonce),
            ("SignatureVersion", "1.0".into()),
            ("TemplateCode", self.cfg.template_code.clone()),
            ("TemplateParam", tpl),
            ("Timestamp", ts),
            ("Version", "2017-05-25".into()),
        ];
        params.sort_by(|a, b| a.0.cmp(b.0));

        let canonical: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencode_aliyun(k), urlencode_aliyun(v)))
            .collect::<Vec<_>>()
            .join("&");
        let string_to_sign = format!("GET&%2F&{}", urlencode_aliyun(&canonical));

        let key = format!("{}&", self.cfg.access_key_secret);
        type HmacSha1 = Hmac<Sha1>;
        let mut mac = HmacSha1::new_from_slice(key.as_bytes()).expect("hmac key");
        mac.update(string_to_sign.as_bytes());
        let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        let mut final_qs = canonical.clone();
        final_qs.push_str(&format!("&Signature={}", urlencode_aliyun(&sig)));
        let url = format!("https://dysmsapi.aliyuncs.com/?{final_qs}");
        let resp = self.client.get(&url).send().await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ChannelError::Upstream(status.as_u16(), body));
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if v.get("Code").and_then(|c| c.as_str()) != Some("OK") {
                return Err(ChannelError::Upstream(200, body));
            }
        }
        Ok(())
    }
}

fn urlencode_aliyun(s: &str) -> String {
    // Aliyun-specific URL encoding: like RFC 3986 but with '+' replaced
    // by '%20' and '*' kept literal? Actually Aliyun says: encode then
    // replace '+' with '%20', '*' with '%2A', '%7E' with '~'.
    let raw: String = s
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    raw.replace('+', "%20")
        .replace('*', "%2A")
        .replace("%7E", "~")
}

fn format_iso8601(secs: u64) -> String {
    let t = time::OffsetDateTime::from_unix_timestamp(secs as i64)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    t.format(&time::format_description::well_known::Iso8601::DEFAULT)
        .unwrap_or_default()
}

//! AWS SNS — Publish via the query API at sns.<region>.amazonaws.com.
//!
//! Auth is Signature Version 4. We hand-roll it (no aws-sdk dep) because
//! the surface we need is tiny: one POST to a known host, form-encoded
//! parameters, no STS, no multi-region failover. SigV4 spec:
//!   https://docs.aws.amazon.com/general/latest/gr/sigv4_signing.html
//!
//! The signature flow:
//!   1. Canonical request: method, path, sorted-canonical query,
//!      canonical headers, signed-headers, hex(sha256(body))
//!   2. String to sign: algorithm, ISO timestamp, scope, hex(sha256(canon))
//!   3. Derive signing key from secret + date + region + service
//!   4. Authorization header includes signature + signed-headers list

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::format_description::well_known::iso8601;
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
pub struct AwsSnsConfig {
    /// SNS topic ARN (preferred) OR phone number for SMS.
    pub topic_arn: Option<String>,
    pub phone_number: Option<String>,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Optional STS session token. Set when the caller is using
    /// temporary credentials (assume-role).
    #[serde(default)]
    pub session_token: Option<String>,
}

pub struct AwsSns {
    cfg: AwsSnsConfig,
    client: reqwest::Client,
}

impl AwsSns {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AwsSnsConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.region.is_empty() || cfg.access_key_id.is_empty() || cfg.secret_access_key.is_empty()
        {
            return Err(ChannelError::BadConfig(
                "region + access_key_id + secret_access_key required".into(),
            ));
        }
        if cfg.topic_arn.is_none() && cfg.phone_number.is_none() {
            return Err(ChannelError::BadConfig(
                "exactly one of topic_arn or phone_number required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Channel for AwsSns {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let host = format!("sns.{}.amazonaws.com", self.cfg.region);
        let service = "sns";
        let now: OffsetDateTime = OffsetDateTime::now_utc();
        let amz_date = now
            .format(
                &iso8601::Iso8601::<
                    {
                        iso8601::Config::DEFAULT
                            .set_year_is_six_digits(false)
                            .set_use_separators(false)
                            .encode()
                    },
                >,
            )
            .map_err(|e| ChannelError::BadConfig(format!("date format: {e}")))?;
        // "YYYYMMDDTHHMMSSZ" — already correct shape; trim millis if present.
        let amz_date = trim_millis(&amz_date);
        let date_stamp = &amz_date[..8];

        // Form-encoded body — sorted by key for a stable canonical request.
        let mut params: Vec<(&str, String)> = vec![
            ("Action", "Publish".to_string()),
            ("Message", body.to_string()),
            ("Subject", subject.to_string()),
            ("Version", "2010-03-31".to_string()),
        ];
        if let Some(arn) = &self.cfg.topic_arn {
            params.push(("TopicArn", arn.clone()));
        }
        if let Some(p) = &self.cfg.phone_number {
            params.push(("PhoneNumber", p.clone()));
        }
        params.sort_by(|a, b| a.0.cmp(b.0));
        let form_body = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let body_hash = hex::encode(Sha256::digest(form_body.as_bytes()));

        // Canonical request.
        let mut canonical_headers = format!(
            "content-type:application/x-www-form-urlencoded\nhost:{host}\nx-amz-date:{amz_date}\n",
        );
        let mut signed_headers = String::from("content-type;host;x-amz-date");
        if let Some(tok) = &self.cfg.session_token {
            canonical_headers.push_str(&format!("x-amz-security-token:{tok}\n"));
            signed_headers.push_str(";x-amz-security-token");
        }
        let canonical = format!("POST\n/\n\n{canonical_headers}\n{signed_headers}\n{body_hash}",);
        let canonical_hash = hex::encode(Sha256::digest(canonical.as_bytes()));

        // String to sign.
        let scope = format!("{date_stamp}/{}/{service}/aws4_request", self.cfg.region);
        let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{canonical_hash}",);

        // Derive signing key.
        let k_date = hmac256(
            format!("AWS4{}", self.cfg.secret_access_key).as_bytes(),
            date_stamp.as_bytes(),
        )?;
        let k_region = hmac256(&k_date, self.cfg.region.as_bytes())?;
        let k_service = hmac256(&k_region, service.as_bytes())?;
        let k_signing = hmac256(&k_service, b"aws4_request")?;
        let signature = hex::encode(hmac256(&k_signing, string_to_sign.as_bytes())?);

        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.cfg.access_key_id,
        );

        let url = format!("https://{host}/");
        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Host", &host)
            .header("X-Amz-Date", &amz_date)
            .header("Authorization", auth)
            .body(form_body);
        if let Some(tok) = &self.cfg.session_token {
            req = req.header("X-Amz-Security-Token", tok);
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

fn hmac256(key: &[u8], msg: &[u8]) -> Result<Vec<u8>, ChannelError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|e| ChannelError::BadConfig(format!("hmac key: {e}")))?;
    mac.update(msg);
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Strip fractional seconds + ensure trailing `Z`. The time crate ISO8601
/// formatter may emit "2026-05-27T17:34:45.123456789Z"; SigV4 wants
/// "20260527T173445Z".
fn trim_millis(s: &str) -> String {
    // Remove dashes + colons, drop fractional seconds.
    let mut out = String::with_capacity(s.len());
    let mut skip_frac = false;
    for c in s.chars() {
        match c {
            '-' | ':' => {}
            '.' => skip_frac = true,
            'Z' => {
                skip_frac = false;
                out.push('Z');
            }
            _ if skip_frac => {}
            _ => out.push(c),
        }
    }
    out
}

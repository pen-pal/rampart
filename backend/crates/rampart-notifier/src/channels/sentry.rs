//! Sentry — post an event to the project DSN.
//! Endpoint derived from the DSN: <protocol>://<host>/api/<project_id>/store/?sentry_key=<public_key>

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use rampart_core::MonitorStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SentryConfig {
    /// Full DSN, e.g. https://abc123@o12345.ingest.sentry.io/67890
    pub dsn: String,
}

pub struct Sentry {
    cfg: SentryConfig,
    client: reqwest::Client,
}

impl Sentry {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: SentryConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.dsn.starts_with("http") {
            return Err(ChannelError::BadConfig("dsn must be http(s) URL".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

fn parse_dsn(dsn: &str) -> Option<(String, String, String)> {
    // <scheme>://<public>@<host>/<project>
    let (scheme, rest) = dsn.split_once("://")?;
    let (public, hostpath) = rest.split_once('@')?;
    let (host, project) = hostpath.rsplit_once('/')?;
    Some((
        format!("{scheme}://{host}"),
        public.to_string(),
        project.to_string(),
    ))
}

#[derive(Serialize)]
struct Payload<'a> {
    message: String,
    level: &'a str,
    logger: &'static str,
    tags: std::collections::HashMap<&'static str, String>,
}

#[async_trait]
impl Channel for Sentry {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let (base, public_key, project) = parse_dsn(&self.cfg.dsn)
            .ok_or_else(|| ChannelError::BadConfig("dsn shape invalid".into()))?;
        let url = format!("{base}/api/{project}/store/?sentry_key={public_key}");
        let level = match event.heartbeat.status {
            MonitorStatus::Up => "info",
            MonitorStatus::Warn => "warning",
            _ => "error",
        };
        let mut tags = std::collections::HashMap::new();
        tags.insert("monitor", event.monitor.name.clone());
        tags.insert("monitor_id", event.monitor.id.0.to_string());
        let payload = Payload {
            message: format!("{subject}\n{body}"),
            level,
            logger: "rampart",
            tags,
        };
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}

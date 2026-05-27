//! PagerDuty via the Events API v2.
//!
//! Setup: in PagerDuty, create a service with "Events API V2" integration
//! type → copy the Integration Key (routing_key).
//!
//! Trigger / Resolve semantics: when the monitor goes DOWN, we send
//! `event_action: "trigger"`; when it goes back UP we send `"resolve"`.
//! We use the monitor id as the `dedup_key` so PagerDuty groups all
//! events for the same monitor into a single incident.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PagerDutyConfig {
    pub routing_key: String,
    /// Override the severity. Default maps from status (down=error, warn=warning).
    #[serde(default)]
    pub severity:    Option<String>,
    /// Optional component string surfaced in the incident.
    #[serde(default)]
    pub component:   Option<String>,
}

#[derive(Debug)]
pub struct PagerDuty {
    cfg:    PagerDutyConfig,
    client: reqwest::Client,
}

impl PagerDuty {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: PagerDutyConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.routing_key.is_empty() {
            return Err(ChannelError::BadConfig("routing_key (integration key) is required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct PdPayload<'a> {
    routing_key:  &'a str,
    event_action: &'a str,
    dedup_key:    String,
    payload:      PdInnerPayload<'a>,
}

#[derive(Serialize)]
struct PdInnerPayload<'a> {
    summary:        &'a str,
    source:         &'a str,
    severity:       &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    component:      Option<&'a str>,
    custom_details: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requires_routing_key() {
        assert!(PagerDuty::from_config(&json!({})).is_err());
    }

    #[test]
    fn accepts_minimal_config() {
        let ok = PagerDuty::from_config(&json!({"routing_key": "abc123"}));
        assert!(ok.is_ok());
    }

    #[test]
    fn accepts_optional_severity_and_component() {
        let ok = PagerDuty::from_config(&json!({
            "routing_key": "abc",
            "severity":    "warning",
            "component":   "payments-api"
        }));
        assert!(ok.is_ok());
    }
}

#[async_trait]
impl Channel for PagerDuty {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let action = match event.heartbeat.status {
            rampart_core::MonitorStatus::Up => "resolve",
            _                               => "trigger",
        };
        let severity = self.cfg.severity.as_deref().unwrap_or(match event.heartbeat.status {
            rampart_core::MonitorStatus::Up   => "info",
            rampart_core::MonitorStatus::Warn => "warning",
            _                                 => "error",
        });

        let payload = PdPayload {
            routing_key:  &self.cfg.routing_key,
            event_action: action,
            dedup_key:    event.monitor.id.0.to_string(),
            payload: PdInnerPayload {
                summary:   subject,
                source:    "rampart",
                severity,
                component: self.cfg.component.as_deref(),
                custom_details: serde_json::json!({
                    "monitor_name": event.monitor.name,
                    "kind":         event.monitor.kind,
                    "url":          event.monitor.url,
                    "status":       event.status_str(),
                    "prev_status":  event.prev_status_str(),
                    "latency_ms":   event.heartbeat.latency_ms,
                    "status_code":  event.heartbeat.status_code,
                    "msg":          event.heartbeat.msg,
                    "body":         body,
                }),
            },
        };

        let resp = self.client
            .post("https://events.pagerduty.com/v2/enqueue")
            .json(&payload)
            .send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}

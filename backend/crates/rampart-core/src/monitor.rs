//! The Monitor — central entity.
//!
//! Slimmed from Rampart v1: dropped multi-region, SLO targets, AI anomaly
//! detection toggles, and auto-failover routing — out-of-scope enterprise
//! features. Twenty probe kinds covered, including a `domain` variant for
//! WHOIS-based expiry checks.

use crate::ids::{MonitorId, ProxyId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

/// All supported probe types.
///
/// Mirrors the `monitor_kind` enum in Postgres. Kind-specific config
/// (e.g. JSONPath expression for `json_query`, container name for
/// `docker`, RRtype for `dns`) lives in `Monitor.config` to avoid
/// schema churn as new probe types land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "monitor_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MonitorKind {
    // HTTP family
    Http,
    Keyword,
    JsonQuery,
    // network primitives
    Tcp,
    Ping,
    Dns,
    Push,
    Grpc,
    Tls,
    // service-specific
    Docker,
    Steam,
    Mqtt,
    Radius,
    Kafka,
    // databases
    Postgres,
    Mysql,
    Mssql,
    Redis,
    Mongodb,
    // registry
    Domain,
}

/// Current rolled-up status of a monitor (or of one heartbeat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "monitor_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum MonitorStatus {
    Up,
    Down,
    Warn,
    Paused,
    /// Never checked yet (just created).
    Pending,
    /// Inside a maintenance window.
    Maintenance,
}

impl MonitorStatus {
    pub fn is_up(self) -> bool { matches!(self, MonitorStatus::Up) }
    pub fn is_down(self) -> bool { matches!(self, MonitorStatus::Down) }
}

/// A live monitor row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monitor {
    pub id:                   MonitorId,
    pub name:                 String,
    pub kind:                 MonitorKind,
    // Endpoint addressing. Which fields are required depends on `kind`;
    // validated at the route layer when accepting NewMonitor.
    pub url:                  Option<String>,
    pub hostname:             Option<String>,
    pub port:                 Option<i32>,
    pub config:               serde_json::Value,
    // Scheduling
    pub interval_seconds:     i32,
    pub retry_interval_sec:   i32,
    pub max_retries:          i32,
    pub timeout_seconds:      i32,
    pub resend_interval_sec:  i32,
    pub upside_down:          bool,
    // HTTP common opts
    pub http_method:          String,
    pub http_body:            Option<String>,
    pub http_headers:         Option<serde_json::Value>,
    pub accepted_statuses:    Vec<i32>,
    pub follow_redirect:      bool,
    pub ignore_tls:           bool,
    pub proxy_id:             Option<ProxyId>,
    // State
    pub active:               bool,
    pub current_status:       MonitorStatus,
    pub created_at:           OffsetDateTime,
    pub updated_at:           OffsetDateTime,
}

/// Payload accepted when creating a monitor. Kind/url/hostname validation
/// is enforced in the route handler — not all combinations make sense.
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewMonitor {
    #[validate(length(min = 1, max = 120))]
    pub name: String,

    pub kind: MonitorKind,

    /// For http/keyword/json_query/tls/domain.
    pub url: Option<String>,

    /// For tcp/ping/dns/grpc/database kinds.
    pub hostname: Option<String>,

    /// For tcp/grpc/database kinds.
    pub port: Option<i32>,

    #[serde(default)]
    pub config: serde_json::Value,

    #[validate(range(min = 10, max = 86400))]
    #[serde(default = "default_interval")]
    pub interval_seconds: i32,

    #[validate(range(min = 1, max = 600))]
    #[serde(default = "default_timeout")]
    pub timeout_seconds: i32,

    #[serde(default)]
    pub max_retries: i32,

    #[serde(default = "default_retry_interval")]
    pub retry_interval_sec: i32,

    #[serde(default)]
    pub resend_interval_sec: i32,

    #[serde(default)]
    pub upside_down: bool,

    #[serde(default = "default_method")]
    pub http_method: String,

    #[serde(default)]
    pub http_body: Option<String>,

    #[serde(default)]
    pub http_headers: Option<serde_json::Value>,

    #[serde(default = "default_accepted_statuses")]
    pub accepted_statuses: Vec<i32>,

    #[serde(default = "default_follow_redirect")]
    pub follow_redirect: bool,

    #[serde(default)]
    pub ignore_tls: bool,

    #[serde(default)]
    pub proxy_id: Option<ProxyId>,
}

fn default_interval()           -> i32 { 60 }
fn default_timeout()            -> i32 { 16 }
fn default_retry_interval()     -> i32 { 60 }
fn default_method()             -> String { "GET".into() }
fn default_follow_redirect()    -> bool { true }
fn default_accepted_statuses()  -> Vec<i32> {
    vec![200,201,202,203,204,205,206,207,208,226]
}

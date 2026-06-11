//! The Monitor — central entity.
//!
//! Slimmed from Rampart v1: dropped multi-region, SLO targets, AI anomaly
//! detection toggles, and auto-failover routing — out-of-scope enterprise
//! features. Twenty probe kinds covered, including a `domain` variant for
//! WHOIS-based expiry checks.

use crate::ids::{AgentId, EscalationPolicyId, MonitorGroupId, MonitorId, ProxyId};
use crate::tag::TagBrief;
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
    /// memcached text-protocol probe — TCP connect, send "version\r\n",
    /// expect server response starting with "VERSION " (configurable).
    Memcached,
    /// NTP probe — sends a minimal SNTPv4 client packet over UDP and
    /// asserts the server responds (mode=server, stratum != 0).
    Ntp,
    /// WebSocket probe — opens an `ws://` / `wss://` handshake and
    /// asserts the server completes RFC 6455's upgrade exchange.
    /// Optional config: `expect` substring required in the first text
    /// frame the server sends (skipped when absent).
    Websocket,
    /// NATS probe — opens a TCP connection to a NATS server, runs the
    /// INFO / CONNECT / PING handshake via the official `async-nats`
    /// client, asserts a successful flush. URL is `nats://host:port`.
    Nats,
    /// LDAP probe — connects to a directory server and runs a simple
    /// bind. URL is `ldap://host:389` or `ldaps://host:636`. Optional
    /// `bind_dn` + `bind_password` config switches from anonymous to
    /// authenticated bind so the probe also validates credentials.
    Ldap,
    /// AMQP 0-9-1 probe (RabbitMQ et al). Opens a TCP connection to the
    /// broker, completes the protocol handshake via the `lapin` client,
    /// closes cleanly. URL is `amqp://user:pass@host:5672/vhost`.
    Amqp,
    /// DNS-over-HTTPS probe (RFC 8484, JSON variant). GETs the DoH
    /// endpoint with `?name=<query>&type=<rtype>`, asserts the response
    /// `Status == 0` (NOERROR) and the `Answer` array is non-empty.
    /// URL is the DoH endpoint (e.g. `https://cloudflare-dns.com/
    /// dns-query`). Config: `query` (default `example.com`), `rtype`
    /// (default `A`).
    Doh,
    /// RDAP probe (RFC 7480 / 9082). Queries a domain via an RDAP
    /// server, asserts a 200 + valid `application/rdap+json` payload,
    /// surfaces days-until-expiry as a soft-fail Warn when present.
    /// URL is the RDAP base (e.g. `https://rdap.org/domain`); config
    /// `domain` carries the lookup target (e.g. `example.com`).
    Rdap,
    /// SNMP v2c GET probe. Issues a single SNMP GET for an OID
    /// against a UDP-reachable agent (default port 161); Up when the
    /// agent replies with a varbind. Same wire format as SNMPv1's GET
    /// — works for both. Hostname + port carry the agent; config
    /// `community` (default `public`) + `oid` (default sysDescr at
    /// `1.3.6.1.2.1.1.1.0`) control the query.
    Snmp,
    /// Cassandra / ScyllaDB probe. Opens a CQL session to the listed
    /// node (default port 9042), runs `SELECT release_version FROM
    /// system.local` to confirm the server is past the CQL handshake
    /// and answering queries. Plaintext only today; TLS deferred.
    Cassandra,
    /// mDNS service-discovery probe (RFC 6762). Sends a multicast
    /// DNS query for `_services._dns-sd._udp.local` (the meta-service
    /// enumerator) to `224.0.0.251:5353` and counts unicast responses
    /// over the timeout window. Up when at least one peer answers.
    Mdns,
    /// SSDP / UPnP service-discovery probe. Sends an HTTP-over-UDP
    /// `M-SEARCH * HTTP/1.1` to `239.255.255.250:1900` and counts
    /// unicast responses. Up when at least one peer answers (e.g.
    /// a smart-TV, NAS, or UPnP-enabled router on the LAN).
    Ssdp,
    // registry
    Domain,
    /// Headless-browser-rendered check. We don't ship a Chromium binary
    /// in the rampart image — operators point this kind at an external
    /// headless service (e.g. browserless/chrome) via config.renderer_url,
    /// then probe asserts on a keyword in the *rendered* HTML.
    Browser,
    // banner protocols — connect + validate the server's greeting line.
    Ssh,
    Smtp,
    Imap,
    Ftp,
    Pop3,
}

/// Per-monitor backoff strategy applied *between* retry attempts after a
/// failing probe, before the failure is committed as a state flip.
///
/// Stored inside the freeform [`Monitor::config`] JSONB under the
/// `retry_backoff` key to avoid a schema change:
///
/// ```json
/// { "retry_backoff": { "strategy": "exponential", "base_secs": 5, "max_secs": 60 } }
/// ```
///
/// When the key is absent, the scheduler does not change its timing at all
/// (identical to the historical behaviour — no inter-retry sleep).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// No growth — every retry waits `base_secs` (the legacy, flat cadence).
    Fixed,
    /// `delay = base_secs × attempt` (attempt is 1-based).
    Linear,
    /// `delay = base_secs × 2^(attempt-1)`.
    Exponential,
}

/// Parsed `config.retry_backoff` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryBackoff {
    pub strategy: BackoffStrategy,
    /// Base delay in seconds. The first retry waits exactly this (for every
    /// strategy, since attempt 1 multiplies base by 1).
    #[serde(default = "default_backoff_base")]
    pub base_secs: u32,
    /// Hard ceiling on the computed delay, in seconds. The scheduler
    /// additionally clamps to the monitor interval at call time.
    #[serde(default = "default_backoff_max")]
    pub max_secs: u32,
}

fn default_backoff_base() -> u32 {
    5
}
fn default_backoff_max() -> u32 {
    60
}

impl RetryBackoff {
    /// Extract the backoff block from a monitor's freeform `config` JSONB.
    /// Returns `None` when the `retry_backoff` key is missing or malformed,
    /// so callers fall back to today's no-sleep behaviour.
    pub fn from_config(config: &serde_json::Value) -> Option<Self> {
        let raw = config.get("retry_backoff")?;
        serde_json::from_value(raw.clone()).ok()
    }

    /// Delay, in seconds, to sleep *before* the given 1-based retry attempt.
    /// `attempt` 1 is the first retry after the initial probe failure.
    ///
    /// The result is clamped to `max_secs`. Computation is saturating so a
    /// large exponent can never overflow or wrap — it simply pins to the cap.
    pub fn delay_secs(&self, attempt: u32) -> u32 {
        let attempt = attempt.max(1);
        let raw = match self.strategy {
            BackoffStrategy::Fixed => self.base_secs,
            BackoffStrategy::Linear => self.base_secs.saturating_mul(attempt),
            BackoffStrategy::Exponential => {
                // base × 2^(attempt-1), saturating. Cap the exponent so the
                // shift can't panic; anything past ~31 already saturates.
                let exp = (attempt - 1).min(31);
                let factor = 1u32.checked_shl(exp).unwrap_or(u32::MAX);
                self.base_secs.saturating_mul(factor)
            }
        };
        raw.min(self.max_secs)
    }
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
    pub fn is_up(self) -> bool {
        matches!(self, MonitorStatus::Up)
    }
    pub fn is_down(self) -> bool {
        matches!(self, MonitorStatus::Down)
    }
}

/// A live monitor row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monitor {
    pub id: MonitorId,
    pub name: String,
    pub kind: MonitorKind,
    // Endpoint addressing. Which fields are required depends on `kind`;
    // validated at the route layer when accepting NewMonitor.
    pub url: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<i32>,
    pub config: serde_json::Value,
    // Scheduling
    pub interval_seconds: i32,
    pub retry_interval_sec: i32,
    pub max_retries: i32,
    pub timeout_seconds: i32,
    pub resend_interval_sec: i32,
    pub upside_down: bool,
    // HTTP common opts
    pub http_method: String,
    pub http_body: Option<String>,
    pub http_headers: Option<serde_json::Value>,
    pub accepted_statuses: Vec<i32>,
    pub follow_redirect: bool,
    pub ignore_tls: bool,
    pub proxy_id: Option<ProxyId>,
    // Push monitor: token in the public /push/:token URL; last_push_at
    // bumps every time we receive a heartbeat through that URL. NULL for
    // monitors of other kinds.
    #[serde(default)]
    pub push_token: Option<String>,
    #[serde(default)]
    pub last_push_at: Option<OffsetDateTime>,
    /// Open run started by a `state=run` ping and not yet closed by
    /// `complete`/`fail`. Lets the completion ping compute the job's
    /// duration and the scheduler flag overruns. Server-managed.
    #[serde(default)]
    pub last_run_started_at: Option<OffsetDateTime>,
    // State
    pub active: bool,
    pub current_status: MonitorStatus,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    /// Tags attached to this monitor. Hydrated by the DB layer on
    /// list/get reads; left empty for serde-default deserialisation
    /// (e.g. test fixtures that don't care about tags).
    #[serde(default)]
    pub tags: Vec<TagBrief>,
    /// TLS cert snapshot — populated for HTTPS HTTP monitors. Null
    /// when never inspected or when the URL is plain http.
    #[serde(default)]
    pub cert_days_left: Option<i32>,
    #[serde(default)]
    pub cert_subject: Option<String>,
    #[serde(default)]
    pub cert_checked_at: Option<OffsetDateTime>,
    /// Optional cosmetic group. NULL → default bucket.
    #[serde(default)]
    pub group_id: Option<MonitorGroupId>,
    /// Optional per-monitor SLO target. `90.000` .. `100.000` when set,
    /// expressed as a percentage. NULL means "no SLO configured" — the
    /// frontend hides the SLO card and the breach detector short-circuits.
    #[serde(default)]
    pub slo_target_pct: Option<f64>,
    /// Rolling SLO window in days. Defaults to 30 at the DB layer; we
    /// keep this `Option` here so the field reads consistently with
    /// `slo_target_pct` (both either set or unset together).
    #[serde(default)]
    pub slo_window_days: Option<i32>,
    /// Remote probe agent this monitor is assigned to. NULL → probed
    /// locally by the in-process scheduler (the default, and the
    /// behaviour of every monitor that predates agents).
    #[serde(default)]
    pub agent_id: Option<AgentId>,
    /// Escalation policy routing this monitor's Down-flips. NULL →
    /// regular attached/tag-routed channel fan-out (the default).
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
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

    #[serde(default)]
    pub group_id: Option<MonitorGroupId>,

    /// Optional SLO target percent (90.0 — 100.0 inclusive).
    #[validate(range(min = 90.0, max = 100.0))]
    #[serde(default)]
    pub slo_target_pct: Option<f64>,

    /// Optional SLO window in days (1 — 90 inclusive). DB-side default
    /// is 30, so callers can leave this unset and still get a sensible
    /// default once an SLO target is supplied.
    #[validate(range(min = 1, max = 90))]
    #[serde(default)]
    pub slo_window_days: Option<i32>,

    /// Assign to a remote probe agent at creation. Omit / null → local.
    #[serde(default)]
    pub agent_id: Option<AgentId>,

    /// Route Down-flips through an escalation policy. Omit / null →
    /// regular channel fan-out.
    #[serde(default)]
    pub escalation_policy_id: Option<EscalationPolicyId>,
}

/// Partial update payload for PATCH /v1/monitors/:id. Every field is
/// optional — fields the caller doesn't send stay untouched. `kind` is
/// deliberately absent: changing kind would invalidate the existing
/// heartbeat history, so we treat it as immutable.
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateMonitor {
    #[validate(length(min = 1, max = 120))]
    pub name: Option<String>,

    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub port: Option<i32>,

    #[serde(default)]
    pub config: Option<serde_json::Value>,

    #[validate(range(min = 10, max = 86400))]
    pub interval_seconds: Option<i32>,

    #[validate(range(min = 1, max = 600))]
    pub timeout_seconds: Option<i32>,

    pub max_retries: Option<i32>,
    pub retry_interval_sec: Option<i32>,
    pub resend_interval_sec: Option<i32>,
    pub upside_down: Option<bool>,

    pub http_method: Option<String>,
    #[serde(default)]
    pub http_body: Option<String>,
    #[serde(default)]
    pub http_headers: Option<serde_json::Value>,
    pub accepted_statuses: Option<Vec<i32>>,
    pub follow_redirect: Option<bool>,
    pub ignore_tls: Option<bool>,

    #[serde(default)]
    pub proxy_id: Option<ProxyId>,

    /// Send `null` to detach from any group; omit to leave unchanged.
    /// Distinguishing "unset" from "explicit null" needs Option<Option<…>>;
    /// callers that want to clear can also set the field to all-zeros UUID
    /// then PATCH again — but the double-option pattern is the cleanest.
    #[serde(default, deserialize_with = "deserialize_optional_group_id")]
    pub group_id: Option<Option<MonitorGroupId>>,

    /// Update the SLO target. Sending `null` clears the SLO; omitting
    /// the field leaves it unchanged. Uses the same Option<Option<…>>
    /// pattern as `group_id` so callers can distinguish "no change"
    /// from "explicitly clear". The 90.0 — 100.0 range is enforced by
    /// the DB CHECK constraint; the route layer validates ahead of the
    /// DB hit so the operator gets a friendlier error message.
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub slo_target_pct: Option<Option<f64>>,

    /// Update the SLO window in days. Like `slo_target_pct` above —
    /// `null` clears, omitted leaves untouched. Range (1 — 90) checked
    /// at the DB layer; route layer validates ahead.
    #[serde(default, deserialize_with = "deserialize_optional_i32")]
    pub slo_window_days: Option<Option<i32>>,

    /// Reassign the probe agent. `null` returns the monitor to local
    /// probing; omitted leaves the assignment unchanged. Same
    /// Option<Option<…>> pattern as `group_id`.
    #[serde(default, deserialize_with = "deserialize_optional_agent_id")]
    pub agent_id: Option<Option<AgentId>>,

    /// Attach/detach an escalation policy. `null` detaches; omitted
    /// leaves unchanged. Option<Option<…>> like `group_id`.
    #[serde(default, deserialize_with = "deserialize_optional_policy_id")]
    pub escalation_policy_id: Option<Option<EscalationPolicyId>>,
}

fn deserialize_optional_policy_id<'de, D>(
    de: D,
) -> Result<Option<Option<EscalationPolicyId>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<EscalationPolicyId> = Option::deserialize(de)?;
    Ok(Some(v))
}

fn deserialize_optional_agent_id<'de, D>(de: D) -> Result<Option<Option<AgentId>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<AgentId> = Option::deserialize(de)?;
    Ok(Some(v))
}

fn deserialize_optional_group_id<'de, D>(de: D) -> Result<Option<Option<MonitorGroupId>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<MonitorGroupId> = Option::deserialize(de)?;
    Ok(Some(v))
}

fn deserialize_optional_f64<'de, D>(de: D) -> Result<Option<Option<f64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<f64> = Option::deserialize(de)?;
    Ok(Some(v))
}

fn deserialize_optional_i32<'de, D>(de: D) -> Result<Option<Option<i32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<i32> = Option::deserialize(de)?;
    Ok(Some(v))
}

fn default_interval() -> i32 {
    60
}
fn default_timeout() -> i32 {
    16
}
fn default_retry_interval() -> i32 {
    60
}
fn default_method() -> String {
    "GET".into()
}
fn default_follow_redirect() -> bool {
    true
}
fn default_accepted_statuses() -> Vec<i32> {
    vec![200, 201, 202, 203, 204, 205, 206, 207, 208, 226]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_kind_serializes_snake_case() {
        // Hot path: the API serializes these to JSON; the DB stores them
        // as a Postgres enum with the same snake_case spelling. They must
        // never drift.
        let cases = [
            (MonitorKind::Http, "http"),
            (MonitorKind::JsonQuery, "json_query"),
            (MonitorKind::Tcp, "tcp"),
            (MonitorKind::Ping, "ping"),
            (MonitorKind::Dns, "dns"),
            (MonitorKind::Push, "push"),
            (MonitorKind::Grpc, "grpc"),
            (MonitorKind::Tls, "tls"),
            (MonitorKind::Postgres, "postgres"),
            (MonitorKind::Mongodb, "mongodb"),
            (MonitorKind::Domain, "domain"),
        ];
        for (k, expected) in cases {
            let v = serde_json::to_string(&k).unwrap();
            assert_eq!(v, format!("\"{expected}\""), "{k:?}");
        }
    }

    #[test]
    fn monitor_kind_deserializes_snake_case() {
        let k: MonitorKind = serde_json::from_str("\"json_query\"").unwrap();
        assert_eq!(k, MonitorKind::JsonQuery);
    }

    #[test]
    fn monitor_status_lowercase_round_trip() {
        for s in [
            MonitorStatus::Up,
            MonitorStatus::Down,
            MonitorStatus::Warn,
            MonitorStatus::Paused,
            MonitorStatus::Pending,
            MonitorStatus::Maintenance,
        ] {
            let v: String = serde_json::to_string(&s).unwrap();
            let back: MonitorStatus = serde_json::from_str(&v).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn monitor_status_helpers() {
        assert!(MonitorStatus::Up.is_up());
        assert!(!MonitorStatus::Up.is_down());
        assert!(MonitorStatus::Down.is_down());
        assert!(!MonitorStatus::Down.is_up());
        assert!(!MonitorStatus::Warn.is_up());
        assert!(!MonitorStatus::Warn.is_down());
    }

    #[test]
    fn new_monitor_applies_serde_defaults() {
        let raw = serde_json::json!({"name": "x", "kind": "http"});
        let nm: NewMonitor = serde_json::from_value(raw).unwrap();
        assert_eq!(nm.interval_seconds, 60);
        assert_eq!(nm.timeout_seconds, 16);
        assert_eq!(nm.http_method, "GET");
        assert!(nm.follow_redirect);
        assert!(!nm.ignore_tls);
        assert_eq!(nm.max_retries, 0);
        assert!(!nm.upside_down);
    }

    #[test]
    fn new_monitor_rejects_too_low_interval() {
        let raw = serde_json::json!({"name": "x", "kind": "http", "interval_seconds": 1});
        let nm: NewMonitor = serde_json::from_value(raw).unwrap();
        use validator::Validate;
        assert!(
            nm.validate().is_err(),
            "interval_seconds < 10 should fail validation"
        );
    }

    #[test]
    fn backoff_linear_grows_and_caps() {
        let b = RetryBackoff {
            strategy: BackoffStrategy::Linear,
            base_secs: 5,
            max_secs: 30,
        };
        // base × attempt
        assert_eq!(b.delay_secs(1), 5);
        assert_eq!(b.delay_secs(2), 10);
        assert_eq!(b.delay_secs(3), 15);
        assert_eq!(b.delay_secs(6), 30); // 5×6=30, exactly at cap
        assert_eq!(b.delay_secs(100), 30); // capped
        assert_eq!(b.delay_secs(0), 5); // attempt clamped to 1
    }

    #[test]
    fn backoff_exponential_grows_and_caps() {
        let b = RetryBackoff {
            strategy: BackoffStrategy::Exponential,
            base_secs: 5,
            max_secs: 60,
        };
        // base × 2^(attempt-1)
        assert_eq!(b.delay_secs(1), 5); // 5×1
        assert_eq!(b.delay_secs(2), 10); // 5×2
        assert_eq!(b.delay_secs(3), 20); // 5×4
        assert_eq!(b.delay_secs(4), 40); // 5×8
        assert_eq!(b.delay_secs(5), 60); // 5×16=80 → capped at 60
        assert_eq!(b.delay_secs(64), 60); // huge exponent saturates then caps
    }

    #[test]
    fn backoff_fixed_is_flat() {
        let b = RetryBackoff {
            strategy: BackoffStrategy::Fixed,
            base_secs: 7,
            max_secs: 60,
        };
        assert_eq!(b.delay_secs(1), 7);
        assert_eq!(b.delay_secs(9), 7);
    }

    #[test]
    fn backoff_parses_from_config_jsonb() {
        let cfg = serde_json::json!({
            "retry_backoff": { "strategy": "exponential", "base_secs": 5, "max_secs": 60 }
        });
        let b = RetryBackoff::from_config(&cfg).expect("should parse");
        assert_eq!(b.strategy, BackoffStrategy::Exponential);
        assert_eq!(b.base_secs, 5);
        assert_eq!(b.max_secs, 60);
    }

    #[test]
    fn backoff_absent_or_malformed_is_none() {
        // No key at all → today's behaviour.
        assert!(RetryBackoff::from_config(&serde_json::json!({})).is_none());
        // Garbage strategy → None, not a panic.
        let bad = serde_json::json!({ "retry_backoff": { "strategy": "wat" } });
        assert!(RetryBackoff::from_config(&bad).is_none());
    }

    #[test]
    fn backoff_uses_defaults_when_fields_omitted() {
        let cfg = serde_json::json!({ "retry_backoff": { "strategy": "linear" } });
        let b = RetryBackoff::from_config(&cfg).unwrap();
        assert_eq!(b.base_secs, 5);
        assert_eq!(b.max_secs, 60);
    }

    #[test]
    fn new_monitor_rejects_empty_name() {
        let raw = serde_json::json!({"name": "", "kind": "http"});
        let nm: NewMonitor = serde_json::from_value(raw).unwrap();
        use validator::Validate;
        assert!(nm.validate().is_err());
    }
}

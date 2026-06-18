// Test-module-in-the-middle-of-the-file is intentional: keeps unit
// tests next to the small helper functions they cover.
#![allow(clippy::items_after_test_module)]

//! Probe runners.
//!
//! Every monitor kind implements [`Probe`]. The scheduler picks the
//! right probe for each monitor, runs it on whatever interval is
//! configured, and writes the resulting [`Heartbeat`] rows.
//!
//! This crate runs probes. It does no scheduling, no persistence, no
//! alerting — those live elsewhere.

pub mod amqp;
pub mod banner;
pub mod browser;
pub mod cassandra;
pub mod dns;
pub mod docker;
pub mod doh;
pub mod domain;
pub mod elasticsearch;
pub mod etcd;
pub mod grpc;
pub mod http;
pub mod kafka;
pub mod ldap;
pub mod mdns;
pub mod memcached;
pub mod mongodb;
pub mod mqtt;
pub mod mssql;
pub mod mysql;
pub mod nats;
pub mod ntp;
pub mod ping;
pub mod postgres;
pub mod radius;
pub mod rdap;
pub mod redis;
pub mod snmp;
pub mod ssdp;
/// SSRF guard moved to the shared `rampart-ssrf` crate (also used by the
/// notifier delivery path). Re-exported so existing `crate::ssrf::*` call sites
/// keep working.
pub use rampart_ssrf as ssrf;
pub mod steam;
pub mod synthetic;
pub mod tcp;
pub mod tls;
pub mod vault;
pub mod websocket;

use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorId, MonitorKind, MonitorStatus};
use std::time::Duration;
use time::OffsetDateTime;

/// Anything that can be probed.
///
/// Implementations should be cheap to construct so we can spin one up
/// per task without thinking about it. Expensive state (HTTP client
/// pools, DNS resolvers) belongs in a shared [`Probes`] handle.
#[async_trait]
pub trait Probe: Send + Sync {
    /// Run one check. Always returns — failures become Heartbeat rows
    /// with a non-Up status, never panics or returns Err.
    async fn run(&self, monitor: &Monitor) -> Heartbeat;
}

/// Bundle of all configured probes. Shares HTTP clients across calls.
pub struct Probes {
    http: http::HttpProbe,
    tcp: tcp::TcpProbe,
    dns: dns::DnsProbe,
    ping: ping::PingProbe,
    tls: tls::TlsProbe,
    domain: domain::DomainProbe,
    postgres: postgres::PostgresProbe,
    mysql: mysql::MySqlProbe,
    mssql: mssql::MssqlProbe,
    redis: redis::RedisProbe,
    mongodb: mongodb::MongodbProbe,
    memcached: memcached::MemcachedProbe,
    ntp: ntp::NtpProbe,
    websocket: websocket::WebsocketProbe,
    nats: nats::NatsProbe,
    ldap: ldap::LdapProbe,
    amqp: amqp::AmqpProbe,
    doh: doh::DohProbe,
    rdap: rdap::RdapProbe,
    snmp: snmp::SnmpProbe,
    cassandra: cassandra::CassandraProbe,
    mdns: mdns::MdnsProbe,
    ssdp: ssdp::SsdpProbe,
    grpc: grpc::GrpcProbe,
    mqtt: mqtt::MqttProbe,
    docker: docker::DockerProbe,
    steam: steam::SteamProbe,
    kafka: kafka::KafkaProbe,
    radius: radius::RadiusProbe,
    browser: browser::BrowserProbe,
    banner: banner::BannerProbe,
    synthetic: synthetic::SyntheticProbe,
    elasticsearch: elasticsearch::ElasticsearchProbe,
    vault: vault::VaultProbe,
    etcd: etcd::EtcdProbe,
}

impl Probes {
    pub fn new() -> Self {
        Self {
            http: http::HttpProbe::new(),
            tcp: tcp::TcpProbe::new(),
            dns: dns::DnsProbe::new(),
            ping: ping::PingProbe::new(),
            tls: tls::TlsProbe::new(),
            domain: domain::DomainProbe::new(),
            postgres: postgres::PostgresProbe::new(),
            mysql: mysql::MySqlProbe::new(),
            mssql: mssql::MssqlProbe::new(),
            redis: redis::RedisProbe::new(),
            mongodb: mongodb::MongodbProbe::new(),
            memcached: memcached::MemcachedProbe::new(),
            ntp: ntp::NtpProbe::new(),
            websocket: websocket::WebsocketProbe::new(),
            nats: nats::NatsProbe::new(),
            ldap: ldap::LdapProbe::new(),
            amqp: amqp::AmqpProbe::new(),
            doh: doh::DohProbe::new(),
            rdap: rdap::RdapProbe::new(),
            snmp: snmp::SnmpProbe::new(),
            cassandra: cassandra::CassandraProbe::new(),
            mdns: mdns::MdnsProbe::new(),
            ssdp: ssdp::SsdpProbe::new(),
            grpc: grpc::GrpcProbe::new(),
            mqtt: mqtt::MqttProbe::new(),
            docker: docker::DockerProbe::new(),
            steam: steam::SteamProbe::new(),
            kafka: kafka::KafkaProbe::new(),
            radius: radius::RadiusProbe::new(),
            browser: browser::BrowserProbe::new(),
            banner: banner::BannerProbe::new(),
            synthetic: synthetic::SyntheticProbe::new(),
            elasticsearch: elasticsearch::ElasticsearchProbe::new(),
            vault: vault::VaultProbe::new(),
            etcd: etcd::EtcdProbe::new(),
        }
    }

    /// HTTP-family probe routed through a proxy. The scheduler calls
    /// this directly when monitor.proxy_id is set; non-HTTP kinds
    /// don't reach this path.
    pub async fn http_with_proxy(
        &self,
        monitor: &Monitor,
        proxy: &rampart_core::proxy::Proxy,
    ) -> Heartbeat {
        self.http.run_with_proxy(monitor, proxy).await
    }

    /// Dispatch to the right probe based on monitor kind. Returns a
    /// Heartbeat with status=Down + descriptive msg for kinds not yet
    /// wired up, so the scheduler doesn't need to know which probes
    /// exist.
    pub async fn run(&self, monitor: &Monitor) -> Heartbeat {
        // Central SSRF guard for connect-based protocol / DB / banner probes.
        // HTTP-family, TCP and synthetic guard themselves (TCP pins the vetted
        // addresses; synthetic is per-step); lookup-only kinds (DNS/Domain/
        // RDAP/DoH), multicast discovery, and hostless kinds (Push/Docker
        // daemon) are exempt. Blocks loopback/link-local/metadata always, and
        // private ranges under RAMPART_SSRF_BLOCK_PRIVATE.
        if let Some((host, port)) = ssrf_target(monitor) {
            if let Err(blocked) =
                ssrf::resolve_guarded(&host, port, ssrf::block_private_enabled()).await
            {
                return ssrf_blocked(monitor.id, &blocked.to_string());
            }
        }
        let hb = match monitor.kind {
            MonitorKind::Http | MonitorKind::Keyword | MonitorKind::JsonQuery => {
                self.http.run(monitor).await
            }
            MonitorKind::Tcp => self.tcp.run(monitor).await,
            MonitorKind::Dns => self.dns.run(monitor).await,
            MonitorKind::Ping => self.ping.run(monitor).await,
            MonitorKind::Tls => self.tls.run(monitor).await,
            MonitorKind::Domain => self.domain.run(monitor).await,
            MonitorKind::Postgres => self.postgres.run(monitor).await,
            MonitorKind::Mysql => self.mysql.run(monitor).await,
            MonitorKind::Mssql => self.mssql.run(monitor).await,
            MonitorKind::Redis => self.redis.run(monitor).await,
            MonitorKind::Mongodb => self.mongodb.run(monitor).await,
            MonitorKind::Memcached => self.memcached.run(monitor).await,
            MonitorKind::Ntp => self.ntp.run(monitor).await,
            MonitorKind::Websocket => self.websocket.run(monitor).await,
            MonitorKind::Nats => self.nats.run(monitor).await,
            MonitorKind::Ldap => self.ldap.run(monitor).await,
            MonitorKind::Amqp => self.amqp.run(monitor).await,
            MonitorKind::Doh => self.doh.run(monitor).await,
            MonitorKind::Rdap => self.rdap.run(monitor).await,
            MonitorKind::Snmp => self.snmp.run(monitor).await,
            MonitorKind::Cassandra => self.cassandra.run(monitor).await,
            MonitorKind::Mdns => self.mdns.run(monitor).await,
            MonitorKind::Ssdp => self.ssdp.run(monitor).await,
            MonitorKind::Grpc => self.grpc.run(monitor).await,
            MonitorKind::Mqtt => self.mqtt.run(monitor).await,
            MonitorKind::Docker => self.docker.run(monitor).await,
            MonitorKind::Steam => self.steam.run(monitor).await,
            MonitorKind::Kafka => self.kafka.run(monitor).await,
            MonitorKind::Radius => self.radius.run(monitor).await,
            MonitorKind::Browser => self.browser.run(monitor).await,
            MonitorKind::Elasticsearch => self.elasticsearch.run(monitor).await,
            MonitorKind::Vault => self.vault.run(monitor).await,
            MonitorKind::Etcd => self.etcd.run(monitor).await,
            MonitorKind::Ssh
            | MonitorKind::Smtp
            | MonitorKind::Imap
            | MonitorKind::Ftp
            | MonitorKind::Pop3 => self.banner.run(monitor).await,
            MonitorKind::Synthetic => self.synthetic.run(monitor).await,
            unsupported => unsupported_kind(monitor.id, unsupported),
        };
        apply_latency_threshold(monitor, hb)
    }
}

/// Soft-SLA latency gate: when `config.max_latency_ms` is set, an otherwise-up
/// check that responded *slower* than the threshold is marked **down** with a
/// "slow" message — degraded detection distinct from the hard connection
/// `timeout_seconds` (which fails the connect/query outright). Applies to every
/// probe kind; a check that's already down, or has no measured latency, is left
/// untouched.
fn apply_latency_threshold(monitor: &Monitor, hb: Heartbeat) -> Heartbeat {
    if hb.status != MonitorStatus::Up {
        return hb;
    }
    let Some(max) = monitor
        .config
        .get("max_latency_ms")
        .and_then(|v| v.as_i64())
        .filter(|&m| m > 0)
    else {
        return hb;
    };
    match hb.latency_ms {
        Some(latency) if (latency as i64) > max => Heartbeat {
            status: MonitorStatus::Down,
            msg: Some(format!("slow: {latency}ms > {max}ms threshold")),
            ..hb
        },
        _ => hb,
    }
}

impl Default for Probes {
    fn default() -> Self {
        Self::new()
    }
}

/// The (host, port) a probe will connect to, for the central SSRF guard, or
/// `None` for kinds that self-guard, only do DNS lookups, or have no remote
/// host. `port` only seeds DNS resolution — the IP block check is port-agnostic.
fn ssrf_target(m: &Monitor) -> Option<(String, u16)> {
    use MonitorKind::*;
    match m.kind {
        // Guarded at the probe level, lookup-only, or no remote connect:
        Http | Keyword | JsonQuery | Tcp | Synthetic | Dns | Domain | Rdap | Doh | Push
        | Browser | Docker | Mdns | Ssdp => None,
        // url-based app-health probes: derive host:port from monitor.url so the
        // central pre-flight guard runs. The reqwest DNS-resolver hook on their
        // guarded client is NOT invoked for a literal-IP URL (e.g.
        // http://169.254.169.254), so the pre-flight `resolve_guarded` — which
        // checks the IP whether literal or resolved — is what actually blocks
        // metadata/loopback targets here.
        Elasticsearch | Vault | Etcd => {
            let url = m.url.as_deref().map(str::trim).filter(|u| !u.is_empty())?;
            let parsed = reqwest::Url::parse(url).ok()?;
            let host = parsed.host_str()?.to_string();
            let port = parsed.port_or_known_default().unwrap_or(443);
            Some((host, port))
        }
        // Everything else connects to monitor.hostname:port.
        _ => {
            let host = m.hostname.clone().filter(|h| !h.trim().is_empty())?;
            let port = m.port.unwrap_or(443).clamp(0, 65535) as u16;
            Some((host, port))
        }
    }
}

fn ssrf_blocked(monitor_id: MonitorId, reason: &str) -> Heartbeat {
    Heartbeat {
        monitor_id,
        ts: OffsetDateTime::now_utc(),
        status: MonitorStatus::Down,
        latency_ms: None,
        status_code: None,
        msg: Some(reason.to_string()),
        retries: 0,
        important: false,
    }
}

fn unsupported_kind(monitor_id: MonitorId, kind: MonitorKind) -> Heartbeat {
    Heartbeat {
        monitor_id,
        ts: OffsetDateTime::now_utc(),
        status: MonitorStatus::Down,
        latency_ms: None,
        status_code: None,
        msg: Some(format!("probe for {kind:?} not yet implemented")),
        retries: 0,
        important: false,
    }
}

/// Saturating ms conversion that fits in i32.
pub(crate) fn ms_i32(d: Duration) -> i32 {
    d.as_millis().min(i32::MAX as u128) as i32
}

#[cfg(test)]
mod latency_tests {
    use super::*;
    use time::OffsetDateTime;

    fn monitor(max_latency: Option<i64>) -> Monitor {
        let now = OffsetDateTime::now_utc();
        Monitor {
            id: MonitorId::new(),
            name: "db".into(),
            kind: MonitorKind::Postgres,
            url: None,
            hostname: Some("db.internal".into()),
            port: Some(5432),
            config: match max_latency {
                Some(m) => serde_json::json!({ "max_latency_ms": m }),
                None => serde_json::Value::Null,
            },
            interval_seconds: 60,
            retry_interval_sec: 60,
            max_retries: 0,
            timeout_seconds: 10,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: None,
            accepted_statuses: vec![],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            push_token: None,
            last_push_at: None,
            last_run_started_at: None,
            active: true,
            current_status: MonitorStatus::Up,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            cert_days_left: None,
            cert_subject: None,
            cert_checked_at: None,
            check_cert: false,
            cert_expiry_days: 14,
            group_id: None,
            slo_target_pct: None,
            slo_window_days: None,
            agent_id: None,
            escalation_policy_id: None,
        }
    }

    fn up(latency_ms: Option<i32>) -> Heartbeat {
        Heartbeat {
            monitor_id: MonitorId::new(),
            ts: OffsetDateTime::now_utc(),
            status: MonitorStatus::Up,
            latency_ms,
            status_code: None,
            msg: Some("ok".into()),
            retries: 0,
            important: false,
        }
    }

    #[test]
    fn no_threshold_keeps_up() {
        let hb = apply_latency_threshold(&monitor(None), up(Some(5000)));
        assert_eq!(hb.status, MonitorStatus::Up);
    }

    #[test]
    fn under_threshold_keeps_up() {
        let hb = apply_latency_threshold(&monitor(Some(1000)), up(Some(800)));
        assert_eq!(hb.status, MonitorStatus::Up);
    }

    #[test]
    fn over_threshold_marks_down() {
        let hb = apply_latency_threshold(&monitor(Some(1000)), up(Some(1500)));
        assert_eq!(hb.status, MonitorStatus::Down);
        assert!(hb.msg.unwrap().contains("slow"));
    }

    #[test]
    fn already_down_untouched() {
        let mut hb = up(Some(9999));
        hb.status = MonitorStatus::Down;
        hb.msg = Some("connect refused".into());
        let out = apply_latency_threshold(&monitor(Some(100)), hb);
        assert_eq!(out.status, MonitorStatus::Down);
        assert_eq!(out.msg.as_deref(), Some("connect refused"));
    }
}

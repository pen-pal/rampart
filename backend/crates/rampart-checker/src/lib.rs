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

pub mod banner;
pub mod browser;
pub mod dns;
pub mod docker;
pub mod domain;
pub mod grpc;
pub mod http;
pub mod kafka;
pub mod memcached;
pub mod mongodb;
pub mod mqtt;
pub mod mssql;
pub mod mysql;
pub mod ntp;
pub mod ping;
pub mod postgres;
pub mod radius;
pub mod redis;
pub mod steam;
pub mod tcp;
pub mod tls;
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
    grpc: grpc::GrpcProbe,
    mqtt: mqtt::MqttProbe,
    docker: docker::DockerProbe,
    steam: steam::SteamProbe,
    kafka: kafka::KafkaProbe,
    radius: radius::RadiusProbe,
    browser: browser::BrowserProbe,
    banner: banner::BannerProbe,
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
            grpc: grpc::GrpcProbe::new(),
            mqtt: mqtt::MqttProbe::new(),
            docker: docker::DockerProbe::new(),
            steam: steam::SteamProbe::new(),
            kafka: kafka::KafkaProbe::new(),
            radius: radius::RadiusProbe::new(),
            browser: browser::BrowserProbe::new(),
            banner: banner::BannerProbe::new(),
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
        match monitor.kind {
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
            MonitorKind::Grpc => self.grpc.run(monitor).await,
            MonitorKind::Mqtt => self.mqtt.run(monitor).await,
            MonitorKind::Docker => self.docker.run(monitor).await,
            MonitorKind::Steam => self.steam.run(monitor).await,
            MonitorKind::Kafka => self.kafka.run(monitor).await,
            MonitorKind::Radius => self.radius.run(monitor).await,
            MonitorKind::Browser => self.browser.run(monitor).await,
            MonitorKind::Ssh
            | MonitorKind::Smtp
            | MonitorKind::Imap
            | MonitorKind::Ftp
            | MonitorKind::Pop3 => self.banner.run(monitor).await,
            unsupported => unsupported_kind(monitor.id, unsupported),
        }
    }
}

impl Default for Probes {
    fn default() -> Self {
        Self::new()
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

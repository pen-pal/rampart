//! TLS certificate probe.
//!
//! Connects to `hostname:port`, completes a TLS handshake against the
//! Mozilla root store, and inspects the leaf certificate. The heartbeat
//! is **Up** if the cert is valid and not expiring within the threshold,
//! **Warn** if it expires soon, and **Down** if the handshake fails or
//! the cert is already expired.
//!
//! Config keys read from `monitor.config`:
//!
//! ```json
//! {
//!   "warn_days": 14,    // Warn if cert expires within this many days
//!   "port":      443    // override default 443
//! }
//! ```
//!
//! `monitor.hostname` is the server name (used for SNI and verification).

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use x509_parser::prelude::{FromDer, X509Certificate};

pub struct TlsProbe {
    client_config: Arc<ClientConfig>,
}

impl TlsProbe {
    pub fn new() -> Self {
        // Mozilla webpki roots; embedded at compile time. We deliberately
        // don't pick up the system trust store — the operator can pin to
        // a custom CA in a future iteration if needed, but matching what
        // the public web sees is what most users actually want here.
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let cfg = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();

        Self {
            client_config: Arc::new(cfg),
        }
    }
}

impl Default for TlsProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for TlsProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "tls monitor requires a hostname");
        };
        let port = monitor
            .config
            .get("port")
            .and_then(|v| v.as_u64())
            .or(monitor.port.map(|p| p as u64))
            .unwrap_or(443) as u16;
        let warn_days = monitor
            .config
            .get("warn_days")
            .and_then(|v| v.as_i64())
            .unwrap_or(14);

        let addr = format!("{host}:{port}");
        let server_name = match ServerName::try_from(host.to_string()) {
            Ok(n) => n,
            Err(e) => return down(monitor, ts, started, &format!("invalid SNI '{host}': {e}")),
        };

        let stream = match timeout(to, TcpStream::connect(&addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("tcp: {e}")),
            Err(_) => return down(monitor, ts, started, "tcp connect timed out"),
        };

        let connector = TlsConnector::from(self.client_config.clone());
        let tls = match timeout(to, connector.connect(server_name, stream)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("handshake: {e}")),
            Err(_) => return down(monitor, ts, started, "tls handshake timed out"),
        };

        let (_, conn) = tls.get_ref();
        let Some(certs) = conn.peer_certificates() else {
            return down(monitor, ts, started, "no certificate presented");
        };
        let Some(leaf) = certs.first() else {
            return down(monitor, ts, started, "empty cert chain");
        };

        match inspect(leaf, warn_days) {
            CertVerdict::Ok(days_left, subject) => Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Up,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some(format!("{subject} — {days_left}d left")),
                retries:     0,
                important:   false,
            },
            CertVerdict::Warn(days_left, subject) => Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Warn,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some(format!("{subject} — expires in {days_left}d")),
                retries:     0,
                important:   false,
            },
            CertVerdict::Expired(days_ago) => down(
                monitor, ts, started,
                &format!("certificate expired {days_ago}d ago"),
            ),
            CertVerdict::ParseError(e) => down(monitor, ts, started, &format!("cert parse: {e}")),
        }
    }
}

fn down(monitor: &Monitor, ts: OffsetDateTime, started: Instant, msg: &str) -> Heartbeat {
    Heartbeat {
        monitor_id:  monitor.id,
        ts,
        status:      MonitorStatus::Down,
        latency_ms:  Some(ms_i32(started.elapsed())),
        status_code: None,
        msg:         Some(msg.into()),
        retries:     0,
        important:   false,
    }
}

enum CertVerdict {
    Ok(i64, String),
    Warn(i64, String),
    Expired(i64),
    ParseError(String),
}

fn inspect(der: &CertificateDer<'_>, warn_days: i64) -> CertVerdict {
    let parsed = match X509Certificate::from_der(der.as_ref()) {
        Ok((_, c)) => c,
        Err(e) => return CertVerdict::ParseError(e.to_string()),
    };
    let not_after = parsed.validity().not_after.timestamp();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let delta_days = (not_after - now) / 86_400;

    let subject = parsed.subject().to_string();
    if delta_days < 0 {
        CertVerdict::Expired(-delta_days)
    } else if delta_days <= warn_days {
        CertVerdict::Warn(delta_days, subject)
    } else {
        CertVerdict::Ok(delta_days, subject)
    }
}

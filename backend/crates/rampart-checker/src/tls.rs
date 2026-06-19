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
        // Threshold precedence: legacy `config.warn_days` wins (so existing
        // tls monitors keep their configured value), otherwise the
        // first-class `cert_expiry_days` column (default 14). For a tls-kind
        // monitor the cert IS the check, so `check_cert` is implicitly true
        // and we always evaluate expiry here.
        let warn_days = monitor
            .config
            .get("warn_days")
            .and_then(|v| v.as_i64())
            .unwrap_or(monitor.cert_expiry_days as i64);

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

        // `ignore_tls` (the same knob the HTTP/NATS probes expose) swaps in a
        // verifier that accepts any certificate. A `tls` monitor's job is to
        // report on the *leaf's expiry*, so an untrusted chain (self-signed /
        // private CA) must not hard-fail the handshake — otherwise expiry can
        // never be evaluated. Trust still applies by default; opt in per-monitor.
        let client_config = if monitor.ignore_tls {
            Arc::new(insecure_client_config())
        } else {
            self.client_config.clone()
        };
        let connector = TlsConnector::from(client_config);
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
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("{subject} — {days_left}d left")),
                retries: 0,
                important: false,
            },
            CertVerdict::Warn(days_left, subject) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Warn,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("{subject} — expires in {days_left}d")),
                retries: 0,
                important: false,
            },
            CertVerdict::Expired(days_ago) => down(
                monitor,
                ts,
                started,
                &format!("certificate expired {days_ago}d ago"),
            ),
            CertVerdict::ParseError(e) => down(monitor, ts, started, &format!("cert parse: {e}")),
        }
    }
}

fn down(monitor: &Monitor, ts: OffsetDateTime, started: Instant, msg: &str) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status: MonitorStatus::Down,
        latency_ms: Some(ms_i32(started.elapsed())),
        status_code: None,
        msg: Some(msg.into()),
        retries: 0,
        important: false,
    }
}

enum CertVerdict {
    Ok(i64, String),
    Warn(i64, String),
    Expired(i64),
    ParseError(String),
}

/// A rustls `ClientConfig` whose server-cert verifier accepts every
/// certificate. Used only when the monitor opts in via `ignore_tls`, so a
/// self-signed / private-CA endpoint still completes the handshake and we can
/// inspect the leaf's expiry. Mirrors the NATS probe's identical knob.
fn insecure_client_config() -> ClientConfig {
    ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertVerification))
        .with_no_client_auth()
}

/// `ServerCertVerifier` impl that approves every offered certificate.
#[derive(Debug)]
struct NoCertVerification;

impl rustls::client::danger::ServerCertVerifier for NoCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

/// Lightweight cert snapshot for the side-band cert info populated on
/// HTTP monitors. (days_left, subject); `days_left` may be negative.
#[derive(Debug, Clone)]
pub struct CertSnapshot {
    pub days_left: i32,
    pub subject: String,
}

/// Connect + inspect, no monitor context. Used by the scheduler to refresh
/// `monitors.cert_days_left` for HTTPS HTTP monitors.
///
/// `insecure` (from a monitor's `ignore_tls`) skips chain verification so the
/// leaf's expiry can still be read off a self-signed / private-CA endpoint —
/// otherwise the handshake fails before we ever see the cert.
pub async fn fetch_cert(
    host: &str,
    port: u16,
    to: Duration,
    insecure: bool,
) -> Result<CertSnapshot, String> {
    let server_name = ServerName::try_from(host.to_string()).map_err(|e| format!("sni: {e}"))?;

    let cfg = if insecure {
        Arc::new(insecure_client_config())
    } else {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    };

    let stream = timeout(to, TcpStream::connect((host, port)))
        .await
        .map_err(|_| "tcp connect timed out".to_string())?
        .map_err(|e| format!("tcp: {e}"))?;
    let tls = timeout(to, TlsConnector::from(cfg).connect(server_name, stream))
        .await
        .map_err(|_| "handshake timed out".to_string())?
        .map_err(|e| format!("handshake: {e}"))?;

    let (_, conn) = tls.get_ref();
    let certs = conn.peer_certificates().ok_or("no certificate presented")?;
    let leaf = certs.first().ok_or("empty cert chain")?;
    let parsed = X509Certificate::from_der(leaf.as_ref())
        .map(|(_, c)| c)
        .map_err(|e| format!("cert parse: {e}"))?;

    let not_after = parsed.validity().not_after.timestamp();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let days_left = ((not_after - now) / 86_400) as i32;
    Ok(CertSnapshot {
        days_left,
        subject: parsed.subject().to_string(),
    })
}

fn inspect(der: &CertificateDer<'_>, warn_days: i64) -> CertVerdict {
    let parsed = match X509Certificate::from_der(der.as_ref()) {
        Ok((_, c)) => c,
        Err(e) => return CertVerdict::ParseError(e.to_string()),
    };
    let not_after = parsed.validity().not_after.timestamp();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let delta_secs = not_after - now;
    // Truncating to whole days first hid certs that expired <24h ago (delta_days
    // rounds to 0, reading as "valid"). Branch on the raw seconds so any past
    // not_after is Expired (Down), not a soft Warn.
    let delta_days = delta_secs / 86_400;

    let subject = parsed.subject().to_string();
    if delta_secs < 0 {
        CertVerdict::Expired(-delta_days)
    } else if delta_days <= warn_days {
        CertVerdict::Warn(delta_days, subject)
    } else {
        CertVerdict::Ok(delta_days, subject)
    }
}

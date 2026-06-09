//! NATS probe.
//!
//! Connects to a NATS server, lets the official `async-nats` client run
//! the INFO / CONNECT / PING handshake, then flushes to confirm a
//! round-trip is healthy. Down if connect fails, the handshake errors,
//! or the flush doesn't return within the monitor timeout.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "name": "rampart-probe",  // client name advertised to the server;
//!                              // defaults to "rampart-checker".
//!   "tls":  true               // require TLS even if the URL is nats://;
//!                              // also auto-enabled when scheme is tls://
//! }
//! ```
//!
//! `monitor.url` carries the target. Two schemes are accepted by
//! `async_nats::ServerAddr`:
//!   * `nats://host:port`  — plaintext STARTTLS-style; the server may
//!     still upgrade if `info.tls_required` arrives in the INFO frame.
//!   * `tls://host:port`   — client mandates TLS from the start; the
//!     handshake fails if the server doesn't speak TLS.
//!
//! The `tls://` path is negotiated through `async-nats`'s built-in
//! rustls connector — the workspace pins async-nats to the `ring`
//! feature (default-features off), so the crypto provider is the
//! workspace-shared ring `CryptoProvider` `rampart_api::main` installs
//! at startup. The alternate `aws-lc-rs` feature is deliberately left
//! disabled (see docs/DEPENDENCIES.md). Root certificates come from
//! `rustls-native-certs`, which async-nats wires in unconditionally for
//! the TLS path and which is already in the workspace tree via
//! bollard / ldap3 / redis — no new dependency.
//!
//! `monitor.ignore_tls` (boolean, present on every Monitor) installs a
//! permissive cert verifier on the rustls `ClientConfig` so operators
//! can probe self-signed clusters in dev. The same knob the HTTP probe
//! advertises; here it materialises as a custom `ServerCertVerifier`
//! that accepts any certificate (no chain validation, no hostname
//! check). NEVER enable this against a public broker — it disables the
//! security guarantee TLS exists to provide.
//!
//! `monitor.timeout_seconds` caps the whole connect-plus-flush window.

use crate::{ms_i32, Probe};
use async_nats::rustls;
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct NatsProbe;

impl NatsProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NatsProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for NatsProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let url = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u,
            _ => {
                return down(
                    monitor,
                    ts,
                    started,
                    "nats monitor requires url (e.g. nats://host:4222 or tls://host:4222)",
                )
            }
        };

        let name = monitor
            .config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("rampart-checker");

        // Two ways to opt into TLS:
        //   1. URL scheme `tls://` — auto-picked up by `ServerAddr`.
        //   2. `config.tls = true` — explicit override for operators
        //      whose URL still says `nats://` but who want the client
        //      to require TLS regardless. The two settings compose; if
        //      either is set, TLS is required.
        let tls_required = monitor
            .config
            .get("tls")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut opts = async_nats::ConnectOptions::new()
            .name(name)
            .connection_timeout(to)
            .require_tls(tls_required);

        // Respect the monitor-level `ignore_tls` knob the HTTP probe
        // also honours. We install a no-verification cert verifier on a
        // rustls `ClientConfig` and hand the whole thing to async-nats —
        // this is the only handle the upstream exposes for relaxing
        // verification (there's no `danger_accept_invalid_certs(true)`
        // shorthand on `ConnectOptions`).
        if monitor.ignore_tls {
            opts = opts.tls_client_config(insecure_client_config());
        }

        let connect_fut = opts.connect(url);

        let client = match timeout(to, connect_fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };

        // `flush()` ensures the client has actually round-tripped a PING
        // / PONG with the server — a successful `connect` alone is too
        // optimistic on some NATS deployments that defer auth failures
        // until the first publish.
        match timeout(to, client.flush()).await {
            Ok(Ok(())) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("connected as {name}")),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("flush: {e}")),
            Err(_) => down(monitor, ts, started, "flush timed out"),
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

/// A rustls `ClientConfig` whose server-cert verifier accepts every
/// certificate. Used only when the monitor opts in via `ignore_tls`.
fn insecure_client_config() -> rustls::ClientConfig {
    rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertVerification))
        .with_no_client_auth()
}

/// `ServerCertVerifier` impl that approves every offered certificate.
/// Mirrors the "danger accept invalid certs" knob `reqwest` exposes for
/// HTTPS — same semantics, same blast radius.
#[derive(Debug)]
struct NoCertVerification;

impl rustls::client::danger::ServerCertVerifier for NoCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        // The full ring-supported set. We never look at the message, so
        // this just keeps the handshake from rejecting outright.
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

#[cfg(test)]
mod tests {
    use async_nats::ServerAddr;
    use std::str::FromStr;

    /// `tls://` is the upstream-defined scheme that signals "client must
    /// negotiate TLS". `ServerAddr::tls_required` reports it back. If a
    /// future refactor accidentally narrows the accepted scheme set
    /// (e.g. by routing through a URL allow-list that only knows
    /// `nats://`), this test fails before reaching the network.
    #[test]
    fn tls_scheme_url_marks_tls_required() {
        let addr =
            ServerAddr::from_str("tls://demo.nats.io:4443").expect("tls:// URL should parse");
        assert!(addr.tls_required());
    }

    /// Mirror of the above: plaintext `nats://` reports `tls_required =
    /// false`. The server's INFO frame can still upgrade the connection
    /// at runtime, but the client doesn't *demand* TLS up-front.
    #[test]
    fn nats_scheme_url_does_not_mark_tls_required() {
        let addr =
            ServerAddr::from_str("nats://demo.nats.io:4222").expect("nats:// URL should parse");
        assert!(!addr.tls_required());
    }
}

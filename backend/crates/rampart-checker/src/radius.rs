//! RADIUS probe — RFC 2865 `Access-Request`.
//!
//! Sends a minimal Access-Request to the server and waits for any
//! reply. Access-Accept (2) → Up, Access-Reject (3) → Up (server is
//! healthy, credentials just wrong — still proves reachability),
//! Access-Challenge (11) → Up. Anything else / no reply → Down.
//!
//! Config:
//!
//! ```json
//! {
//!   "secret":   "testing123",   // required: shared secret
//!   "username": "probe",
//!   "password": "probe-pass"
//! }
//! ```
//!
//! User-Password attribute is encrypted exactly per § 5.2 of the RFC:
//! split into 16-byte chunks, each chunk XOR'd with MD5(secret || prev).

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use md5::{Digest, Md5};
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use rand::RngCore;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::net::UdpSocket;
use tokio::time::timeout;

const CODE_ACCESS_REQUEST:   u8 = 1;
const CODE_ACCESS_ACCEPT:    u8 = 2;
const CODE_ACCESS_REJECT:    u8 = 3;
const CODE_ACCESS_CHALLENGE: u8 = 11;

const ATTR_USER_NAME:     u8 = 1;
const ATTR_USER_PASSWORD: u8 = 2;

pub struct RadiusProbe;

impl RadiusProbe {
    pub fn new() -> Self { Self }
}
impl Default for RadiusProbe {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Probe for RadiusProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "radius monitor requires hostname");
        };
        let port = monitor.port.unwrap_or(1812) as u16;

        let secret = monitor.config.get("secret").and_then(|v| v.as_str());
        let Some(secret) = secret else {
            return down(monitor, ts, started, "radius monitor requires config.secret");
        };
        let username = monitor.config.get("username").and_then(|v| v.as_str()).unwrap_or("probe");
        let password = monitor.config.get("password").and_then(|v| v.as_str()).unwrap_or("probe");

        // 16-byte random Request-Authenticator.
        let mut authenticator = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut authenticator);
        let identifier: u8 = rand::random();

        let packet = build_access_request(
            identifier,
            &authenticator,
            username,
            password,
            secret,
        );

        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => return down(monitor, ts, started, &format!("udp bind: {e}")),
        };
        if let Err(e) = socket.connect((host, port)).await {
            return down(monitor, ts, started, &format!("udp connect: {e}"));
        }
        if let Err(e) = socket.send(&packet).await {
            return down(monitor, ts, started, &format!("send: {e}"));
        }

        let mut buf = [0u8; 4096];
        let n = match timeout(to, socket.recv(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("recv: {e}")),
            Err(_) => return down(monitor, ts, started, "no reply"),
        };

        if n < 20 {
            return down(monitor, ts, started, "short reply");
        }
        if buf[1] != identifier {
            return down(monitor, ts, started, "identifier mismatch");
        }

        let msg = match buf[0] {
            CODE_ACCESS_ACCEPT    => "Access-Accept",
            CODE_ACCESS_REJECT    => "Access-Reject",
            CODE_ACCESS_CHALLENGE => "Access-Challenge",
            other => {
                return down(monitor, ts, started, &format!("unexpected code {other}"));
            }
        };
        // All three codes prove the server is up. A Reject just means
        // the canned probe creds don't authenticate — which is fine; we
        // care about server reachability, not credentials.
        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: MonitorStatus::Up,
            latency_ms: Some(ms_i32(started.elapsed())),
            status_code: None,
            msg: Some(msg.into()),
            retries: 0,
            important: false,
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

fn build_access_request(
    identifier: u8,
    authenticator: &[u8; 16],
    username: &str,
    password: &str,
    secret: &str,
) -> Vec<u8> {
    let user_attr   = build_attr(ATTR_USER_NAME, username.as_bytes());
    let pwd_encoded = encrypt_password(password, secret, authenticator);
    let pwd_attr    = build_attr(ATTR_USER_PASSWORD, &pwd_encoded);

    let length = 20 + user_attr.len() + pwd_attr.len();
    let mut pkt = Vec::with_capacity(length);
    pkt.push(CODE_ACCESS_REQUEST);
    pkt.push(identifier);
    pkt.extend_from_slice(&(length as u16).to_be_bytes());
    pkt.extend_from_slice(authenticator);
    pkt.extend_from_slice(&user_attr);
    pkt.extend_from_slice(&pwd_attr);
    pkt
}

fn build_attr(kind: u8, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + value.len());
    out.push(kind);
    out.push((2 + value.len()) as u8);
    out.extend_from_slice(value);
    out
}

/// RFC 2865 § 5.2 User-Password encryption.
fn encrypt_password(password: &str, secret: &str, authenticator: &[u8; 16]) -> Vec<u8> {
    // Zero-pad password to next 16-byte boundary (minimum 16).
    let mut plain = password.as_bytes().to_vec();
    let pad = 16 - (plain.len() % 16);
    if plain.is_empty() || pad != 16 {
        plain.extend(std::iter::repeat(0u8).take(pad));
    }

    let mut prev: [u8; 16] = *authenticator;
    let mut out = Vec::with_capacity(plain.len());

    for chunk in plain.chunks(16) {
        let mut hasher = Md5::new();
        hasher.update(secret.as_bytes());
        hasher.update(prev);
        let digest = hasher.finalize();

        let mut block = [0u8; 16];
        for i in 0..16 {
            block[i] = chunk[i] ^ digest[i];
        }
        out.extend_from_slice(&block);
        prev = block;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_pads_to_16() {
        let auth = [0u8; 16];
        let out = encrypt_password("short", "secret", &auth);
        assert_eq!(out.len(), 16);
    }
    #[test]
    fn password_longer_than_16_chunks() {
        let auth = [0u8; 16];
        let out = encrypt_password(&"a".repeat(20), "secret", &auth);
        assert_eq!(out.len(), 32);
    }
    #[test]
    fn access_request_has_minimum_length() {
        let auth = [0u8; 16];
        let pkt = build_access_request(7, &auth, "alice", "pw", "secret");
        assert_eq!(pkt[0], CODE_ACCESS_REQUEST);
        assert_eq!(pkt[1], 7);
        assert!(pkt.len() >= 20 + 7 /* User-Name attr */ + 18 /* pwd attr */);
    }
}

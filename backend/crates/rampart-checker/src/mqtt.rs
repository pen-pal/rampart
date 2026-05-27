//! MQTT probe — connect to a broker, expect a successful CONNACK.
//!
//! Doesn't subscribe — broker handshake alone covers AUTH + reachability,
//! which is what 95% of MQTT outages actually break. Topic-level
//! subscribe / receive can land later when monitor.config grows a
//! `topic` field.
//!
//! Config:
//!
//! ```json
//! {
//!   "username": "ha",
//!   "password": "secret",
//!   "tls":      false,
//!   "client_id": "rampart-probe"
//! }
//! ```

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, Transport};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct MqttProbe;

impl MqttProbe {
    pub fn new() -> Self { Self }
}
impl Default for MqttProbe {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Probe for MqttProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "mqtt monitor requires hostname");
        };
        let port = monitor.port.unwrap_or(1883) as u16;
        let cid = monitor
            .config
            .get("client_id")
            .and_then(|v| v.as_str())
            .unwrap_or("rampart-probe");

        let mut opts = MqttOptions::new(cid, host, port);
        opts.set_keep_alive(Duration::from_secs(15));
        opts.set_clean_session(true);
        if let (Some(u), Some(p)) = (
            monitor.config.get("username").and_then(|v| v.as_str()),
            monitor.config.get("password").and_then(|v| v.as_str()),
        ) {
            opts.set_credentials(u, p);
        }
        let use_tls = monitor.config.get("tls").and_then(|v| v.as_bool()).unwrap_or(false);
        if use_tls {
            opts.set_transport(Transport::tls_with_default_config());
        }

        let (client, mut eventloop) = AsyncClient::new(opts, 8);

        // Pump events until we see a ConnAck or hit the deadline. rumqttc
        // drives the connection from poll(), so we have to spin it.
        let waiter = async {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(ack))) => return Ok(ack),
                    Ok(_) => continue,
                    Err(e) => return Err(format!("{e}")),
                }
            }
        };

        let outcome = timeout(to, waiter).await;
        // Best-effort tidy disconnect; ignore errors.
        let _ = client.disconnect().await;

        match outcome {
            Ok(Ok(ack)) => {
                use rumqttc::ConnectReturnCode::*;
                let (s, msg) = match ack.code {
                    Success => (MonitorStatus::Up, "ConnAck Success".to_string()),
                    code => (MonitorStatus::Down, format!("ConnAck {:?}", code)),
                };
                Heartbeat {
                    monitor_id: monitor.id,
                    ts,
                    status: s,
                    latency_ms: Some(ms_i32(started.elapsed())),
                    status_code: None,
                    msg: Some(msg),
                    retries: 0,
                    important: false,
                }
            }
            Ok(Err(e)) => down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => down(monitor, ts, started, "connect timed out"),
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

// Silence unused-import lint when QoS isn't referenced elsewhere — keeps
// the file ready to grow a subscribe/receive code path.
#[allow(dead_code)]
fn _qos_marker(_: QoS) {}

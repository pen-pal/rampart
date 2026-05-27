//! MongoDB probe — connect, run `{ping: 1}` against `admin`.
//!
//! Connection URL precedence: `monitor.config.connection_string`,
//! otherwise built as `mongodb://[user[:pass]@]host[:port]`.
//! For replica sets and other non-trivial topologies, use the raw
//! connection string; the synthesizer here is a convenience for the
//! single-instance case.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use mongodb::bson::{doc, Document};
use mongodb::options::ClientOptions;
use mongodb::Client;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct MongodbProbe;

impl MongodbProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MongodbProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for MongodbProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let uri = match build_uri(monitor) {
            Ok(u) => u,
            Err(e) => return down(monitor, ts, started, &e),
        };

        let parse_fut = ClientOptions::parse(&uri);
        let mut opts = match timeout(to, parse_fut).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("uri parse: {e}")),
            Err(_) => return down(monitor, ts, started, "uri parse timed out"),
        };
        // Bound the driver's own internal timeouts so we don't sit past
        // the probe deadline.
        opts.server_selection_timeout = Some(to);
        opts.connect_timeout = Some(to);

        let client = match Client::with_options(opts) {
            Ok(c) => c,
            Err(e) => return down(monitor, ts, started, &format!("client: {e}")),
        };

        let cmd: Document = doc! { "ping": 1 };
        let db = client.database("admin");
        let run = db.run_command(cmd);
        match timeout(to, run).await {
            Ok(Ok(_)) => Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Up,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some("ping ok".into()),
                retries:     0,
                important:   false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("ping: {e}")),
            Err(_) => down(monitor, ts, started, "ping timed out"),
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

fn build_uri(monitor: &Monitor) -> Result<String, String> {
    if let Some(s) = monitor
        .config
        .get("connection_string")
        .and_then(|v| v.as_str())
    {
        if !s.is_empty() {
            return Ok(s.to_string());
        }
    }
    let host = monitor
        .hostname
        .as_deref()
        .ok_or_else(|| "mongodb monitor requires hostname or connection_string".to_string())?;
    let port = monitor.port.unwrap_or(27017) as u16;

    let creds = match (
        monitor.config.get("user").and_then(|v| v.as_str()),
        monitor.config.get("password").and_then(|v| v.as_str()),
    ) {
        (Some(u), Some(p)) if !u.is_empty() => format!("{u}:{p}@"),
        (Some(u), None) if !u.is_empty() => format!("{u}@"),
        _ => String::new(),
    };
    Ok(format!("mongodb://{creds}{host}:{port}"))
}

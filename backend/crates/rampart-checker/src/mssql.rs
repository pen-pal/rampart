//! MSSQL probe — TDS connection via tiberius, run `SELECT 1`.
//!
//! Tiberius is written against `futures-io`, so we adapt the tokio
//! TcpStream via `tokio_util::compat`. Config keys (all optional, with
//! sensible defaults — the user, password, and database all come from
//! `monitor.config`):
//!
//! ```json
//! {
//!   "user":     "sa",
//!   "password": "secret",
//!   "database": "master",
//!   "trust_cert": true     // default true; flip to false for prod
//! }
//! ```

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use tiberius::{AuthMethod, Client, Config};
use time::OffsetDateTime;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::compat::TokioAsyncWriteCompatExt;

pub struct MssqlProbe;

impl MssqlProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MssqlProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for MssqlProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(host) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "mssql monitor requires a hostname");
        };
        let port = monitor.port.unwrap_or(1433) as u16;

        let user = monitor
            .config
            .get("user")
            .and_then(|v| v.as_str())
            .unwrap_or("sa")
            .to_string();
        let pass = monitor
            .config
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let database = monitor
            .config
            .get("database")
            .and_then(|v| v.as_str())
            .unwrap_or("master")
            .to_string();
        // Default to trusting because most lab deployments use self-signed
        // certs. Production users can flip this.
        let trust_cert = monitor
            .config
            .get("trust_cert")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let mut cfg = Config::new();
        cfg.host(host);
        cfg.port(port);
        cfg.authentication(AuthMethod::sql_server(&user, &pass));
        cfg.database(database);
        if trust_cert {
            cfg.trust_cert();
        }

        let tcp = match timeout(to, TcpStream::connect((host, port))).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("tcp: {e}")),
            Err(_) => return down(monitor, ts, started, "tcp connect timed out"),
        };
        if let Err(e) = tcp.set_nodelay(true) {
            return down(monitor, ts, started, &format!("set_nodelay: {e}"));
        }

        let connect_fut = Client::connect(cfg, tcp.compat_write());
        let mut client = match timeout(to, connect_fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("tds: {e}")),
            Err(_) => return down(monitor, ts, started, "tds handshake timed out"),
        };

        let result = {
            let query_fut = client.simple_query("SELECT 1");
            timeout(to, query_fut).await
        };
        match result {
            Ok(Ok(_)) => Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Up,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some("SELECT 1 ok".into()),
                retries:     0,
                important:   false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("query: {e}")),
            Err(_) => down(monitor, ts, started, "query timed out"),
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

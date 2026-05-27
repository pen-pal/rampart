//! Postgres probe — open a connection, run `SELECT 1`, time it.
//!
//! Connection string sources, in priority order:
//! 1. `monitor.config.connection_string` — full URL the user provided.
//! 2. Synthesized from `hostname` + `port` + `monitor.config.{user,password,database}`.
//!
//! We deliberately don't keep a pool here. Each probe opens a fresh
//! connection so a slow tear-down on the server side surfaces as a
//! down monitor instead of being hidden behind pool reuse.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use sqlx::{Connection, Executor, PgConnection};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct PostgresProbe;

impl PostgresProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PostgresProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for PostgresProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let conn_str = match build_conn_str(monitor, "postgres", 5432) {
            Ok(s) => s,
            Err(e) => return down(monitor, ts, started, &e),
        };

        let connect_fut = PgConnection::connect(&conn_str);
        let mut conn = match timeout(to, connect_fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };

        let query_fut = conn.execute("SELECT 1");
        let result = timeout(to, query_fut).await;
        // Always try to close cleanly; failures here don't change the verdict.
        let _ = conn.close().await;

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

/// Shared connection-string builder for postgres/mysql probes. Tries the
/// raw `connection_string` config first, then falls back to assembling
/// from individual fields. `scheme` is `"postgres"` or `"mysql"`.
pub(crate) fn build_conn_str(
    monitor: &Monitor,
    scheme: &str,
    default_port: u16,
) -> Result<String, String> {
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
        .ok_or_else(|| format!("{scheme} monitor requires hostname or connection_string"))?;
    let port = monitor.port.map(|p| p as u16).unwrap_or(default_port);

    let user = monitor
        .config
        .get("user")
        .and_then(|v| v.as_str())
        .unwrap_or(scheme);
    let pass = monitor
        .config
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let db = monitor
        .config
        .get("database")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let creds = if pass.is_empty() {
        user.to_string()
    } else {
        format!("{user}:{pass}")
    };
    Ok(format!("{scheme}://{creds}@{host}:{port}/{db}"))
}

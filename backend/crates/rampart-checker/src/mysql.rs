//! MySQL / MariaDB probe — open a connection, run `SELECT 1`, time it.
//!
//! Same connection-string contract as the postgres probe — see
//! [`crate::postgres::build_conn_str`] for the rules.

use crate::postgres::build_conn_str;
use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use sqlx::{Connection, Executor, MySqlConnection};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct MySqlProbe;

impl MySqlProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MySqlProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for MySqlProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let conn_str = match build_conn_str(monitor, "mysql", 3306) {
            Ok(s) => s,
            Err(e) => return down(monitor, ts, started, &e),
        };

        let mut conn = match timeout(to, MySqlConnection::connect(&conn_str)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };

        let result = timeout(to, conn.execute("SELECT 1")).await;
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

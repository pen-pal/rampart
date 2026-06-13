//! Single-leader election via a Postgres session advisory lock.
//!
//! In a multi-replica deployment only ONE instance may run the probe
//! scheduler, notifier digest flush, escalation timers and the retention
//! prune. Otherwise every replica probes every monitor (N× load) and fires
//! duplicate alerts. We coordinate with `pg_try_advisory_lock` held on a
//! dedicated connection for the life of the leader: when the leader process
//! dies its session ends and Postgres frees the lock, so a standby acquires
//! it on its next attempt (failover within `RETRY`). A connection drop on a
//! still-running leader is detected by the keepalive and flips it back to
//! follower, so it stops the background work before another node takes over.
//!
//! The HTTP API runs on every replica regardless of leadership — only the
//! background loops gate on [`Leadership::is_leader`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sqlx::Connection;
use tracing::{info, warn};

/// Fixed advisory-lock key, unique to Rampart's scheduler ("RAMP" = 0x52414D50).
const LOCK_KEY: i64 = 0x5241_4D50;
/// How often a follower retries acquisition (and the failover ceiling).
const RETRY: Duration = Duration::from_secs(10);
/// How often the holder pings to detect a dropped connection.
const KEEPALIVE: Duration = Duration::from_secs(15);

/// Shared leadership flag. Cheap to clone (Arc) and read (atomic).
#[derive(Default)]
pub struct Leadership {
    leader: AtomicBool,
}

impl Leadership {
    /// True while this process holds the scheduler lock.
    pub fn is_leader(&self) -> bool {
        self.leader.load(Ordering::Relaxed)
    }

    /// A leadership handle that is always leader. For single-process tests /
    /// harnesses that never run the election loop.
    pub fn always() -> Arc<Self> {
        let l = Arc::new(Self::default());
        l.leader.store(true, Ordering::Relaxed);
        l
    }
}

/// Spawn the election loop. The returned handle flips to leader once this
/// process holds the advisory lock; background loops should gate on it.
pub fn spawn(database_url: String) -> Arc<Leadership> {
    let handle = Arc::new(Leadership::default());
    let task = handle.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = hold_session(&database_url, &task).await {
                warn!(error = %e, "leader session ended; will re-elect");
            }
            task.leader.store(false, Ordering::Relaxed);
            tokio::time::sleep(RETRY).await;
        }
    });
    handle
}

/// Open a dedicated connection, acquire the lock (polling until granted),
/// then hold it — pinging periodically so a dropped connection surfaces as
/// an error and re-election kicks in. Returns when the session is lost.
async fn hold_session(url: &str, task: &Leadership) -> Result<(), sqlx::Error> {
    let mut conn = sqlx::postgres::PgConnection::connect(url).await?;
    loop {
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(LOCK_KEY)
            .fetch_one(&mut conn)
            .await?;
        if acquired {
            break;
        }
        // Another replica leads. Wait, then retry on the same connection.
        tokio::time::sleep(RETRY).await;
    }
    task.leader.store(true, Ordering::Relaxed);
    info!("acquired scheduler leadership (pg advisory lock)");
    loop {
        tokio::time::sleep(KEEPALIVE).await;
        // Errors if the connection dropped → propagate to re-elect.
        sqlx::query("SELECT 1").execute(&mut conn).await?;
    }
}

/// The advisory-lock key, exposed so integration tests can assert the
/// exclusivity invariant the election relies on.
pub const ADVISORY_LOCK_KEY: i64 = LOCK_KEY;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_follower_always_is_leader() {
        let l = Leadership::default();
        assert!(!l.is_leader());
        let a = Leadership::always();
        assert!(a.is_leader());
    }
}

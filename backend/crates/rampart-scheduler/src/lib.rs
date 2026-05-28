//! Rampart scheduler.
//!
//! Drives all active monitors. For each, spawns a dedicated tokio task
//! that ticks on the monitor's interval, invokes the right probe, and
//! sends the resulting `Heartbeat` down a channel to a batching writer.
//!
//! Design choices:
//!
//! - **One task per monitor.** Simpler than a global ticker + work
//!   queue. A homelab won't run more than a few hundred monitors;
//!   tokio handles that without thinking.
//!
//! - **Batched writes.** Each heartbeat is an INSERT, but the writer
//!   collects up to 256 of them or 1 second of wall time, whichever
//!   comes first, and flushes via `heartbeats::insert_many`. Cuts
//!   round-trips by ~100x at scale.
//!
//! - **Status-flip detection.** The scheduler keeps the previous
//!   status in-memory per monitor and sets `important = true` on the
//!   heartbeat at the moment the status flips. That's how the UI shows
//!   "outage started 14:32" without scanning the whole series.
//!
//! - **Reload-on-change.** A `reload()` signal causes the scheduler to
//!   diff current monitors against the database and start/stop tasks
//!   accordingly. Called from the API after monitor create/delete/edit.

use rampart_checker::Probes;
use rampart_core::{Heartbeat, Monitor, MonitorId, MonitorKind, MonitorStatus};
use rampart_db::DbPool;
use rampart_notifier::{Event, EventKind, NotifierHandle};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Notify, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// Maximum heartbeats batched into one INSERT, OR
/// maximum wall-clock between flushes, whichever first.
const BATCH_SIZE: usize = 256;
const BATCH_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// Channel buffer between probe tasks and the writer.
const HEARTBEAT_CHANNEL_BUFFER: usize = 4096;
/// Broadcast ring depth for live SSE subscribers. Lagging consumers get
/// `RecvError::Lagged` past this; the SSE endpoint logs + keeps going.
const HEARTBEAT_BROADCAST_DEPTH: usize = 256;

pub struct Scheduler {
    pool: DbPool,
    probes: Arc<Probes>,
    /// Map from MonitorId → handle of its probe task.
    tasks: Arc<RwLock<HashMap<MonitorId, MonitorTask>>>,
    /// Sender side of the heartbeat channel — cloned into each probe task.
    hb_tx: mpsc::Sender<Heartbeat>,
    /// Live-stream fanout. Subscribers see heartbeats *after* they're
    /// persisted, so the API never streams data the DB doesn't yet have.
    hb_broadcast: broadcast::Sender<Heartbeat>,
    /// Bumped to wake the reload loop after a monitor mutation.
    reload: Arc<Notify>,
    /// Optional notifier handle — when present, status-flip events get
    /// pushed onto its queue for fan-out. None disables notifications.
    notifier: Option<NotifierHandle>,
}

struct MonitorTask {
    handle: JoinHandle<()>,
    /// Bumping this notify cancels the probe loop on the next tick.
    cancel: Arc<Notify>,
    /// Last status we observed for this monitor. The probe task owns
    /// the *write* side via its own clone; we keep this here so the
    /// scheduler can read it back without going through the DB if it
    /// ever needs to (currently unused but cheap to retain).
    #[allow(dead_code)]
    last_status: Arc<RwLock<MonitorStatus>>,
}

impl Scheduler {
    /// Construct and immediately spawn the writer task. Call
    /// [`Scheduler::run`] afterward to kick off the reload loop.
    pub fn new(pool: DbPool) -> Self {
        Self::with_notifier(pool, None)
    }

    pub fn with_notifier(pool: DbPool, notifier: Option<NotifierHandle>) -> Self {
        let (hb_tx, hb_rx) = mpsc::channel::<Heartbeat>(HEARTBEAT_CHANNEL_BUFFER);
        let (hb_broadcast, _) = broadcast::channel::<Heartbeat>(HEARTBEAT_BROADCAST_DEPTH);
        let writer_pool = pool.clone();
        let writer_broadcast = hb_broadcast.clone();
        tokio::spawn(async move {
            writer_loop(writer_pool, hb_rx, writer_broadcast).await;
        });

        Self {
            pool,
            probes: Arc::new(Probes::new()),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            hb_tx,
            hb_broadcast,
            reload: Arc::new(Notify::new()),
            notifier,
        }
    }

    /// Returns a handle that callers can use to trigger a reload after
    /// changing monitors via the API.
    pub fn reload_handle(&self) -> Arc<Notify> {
        self.reload.clone()
    }

    /// Subscribe to the live heartbeat stream. Each receiver gets every
    /// heartbeat post-persist, with a bounded backlog — slow consumers
    /// receive `RecvError::Lagged` rather than blocking the writer.
    pub fn subscribe_heartbeats(&self) -> broadcast::Receiver<Heartbeat> {
        self.hb_broadcast.subscribe()
    }

    /// Run forever. Reconciles the running task set against the DB on
    /// every reload signal, and on a slow timer as a fallback.
    pub async fn run(self: Arc<Self>) {
        // Initial reconcile so existing monitors start probing immediately.
        if let Err(e) = self.reconcile().await {
            error!(error = %e, "initial reconcile failed");
        }
        let slow_tick = Duration::from_secs(30);

        loop {
            tokio::select! {
                _ = self.reload.notified() => {}
                _ = tokio::time::sleep(slow_tick) => {}
            }
            if let Err(e) = self.reconcile().await {
                error!(error = %e, "reconcile failed");
            }
        }
    }

    /// Diff active monitors in DB against in-memory tasks. Start tasks
    /// for monitors that should be running but aren't; cancel tasks for
    /// monitors that have been deleted or deactivated.
    async fn reconcile(&self) -> Result<(), rampart_db::DbError> {
        let live = rampart_db::monitors::list(&self.pool).await?;
        let live_active: HashMap<MonitorId, Monitor> = live
            .into_iter()
            .filter(|m| m.active)
            .map(|m| (m.id, m))
            .collect();

        // First pass: cancel any task whose monitor is no longer active.
        let to_cancel: Vec<MonitorId> = {
            let tasks = self.tasks.read().await;
            tasks
                .keys()
                .copied()
                .filter(|id| !live_active.contains_key(id))
                .collect()
        };
        for id in to_cancel {
            self.stop_task(id).await;
        }

        // Second pass: start tasks for monitors that aren't running yet.
        let running: std::collections::HashSet<MonitorId> =
            { self.tasks.read().await.keys().copied().collect() };
        for (id, monitor) in live_active {
            if !running.contains(&id) {
                self.start_task(monitor).await;
            }
        }
        Ok(())
    }

    async fn start_task(&self, monitor: Monitor) {
        let cancel = Arc::new(Notify::new());
        let last_status = Arc::new(RwLock::new(monitor.current_status));

        let cancel_in_task = cancel.clone();
        let probes = self.probes.clone();
        let hb_tx = self.hb_tx.clone();
        let last_status_in_task = last_status.clone();
        let notifier = self.notifier.clone();
        let pool_in_task = self.pool.clone();
        let monitor_id = monitor.id;
        let monitor_name = monitor.name.clone();
        let initial_interval = monitor.interval_seconds;

        let handle = tokio::spawn(async move {
            info!(monitor = %monitor_id, name = %monitor_name, interval_s = initial_interval, "probe task started");

            // Use the freshly-passed monitor for the immediate first
            // fire so the dashboard reflects state quickly.
            run_once(
                &probes,
                &monitor,
                &last_status_in_task,
                &hb_tx,
                notifier.as_ref(),
                &pool_in_task,
            )
            .await;

            let mut interval = Duration::from_secs(initial_interval as u64);
            loop {
                tokio::select! {
                    _ = cancel_in_task.notified() => {
                        info!(monitor = %monitor_id, "probe task cancelled");
                        return;
                    }
                    _ = tokio::time::sleep(interval) => {
                        // Re-fetch every tick so PATCH-edited fields
                        // (config blob, url, timeout, interval) take
                        // effect on the very next probe rather than only
                        // after a delete + recreate. Single PK lookup;
                        // cheap even at thousands of monitors.
                        match rampart_db::monitors::get(&pool_in_task, monitor_id).await {
                            Ok(fresh) => {
                                if !fresh.active {
                                    // Reconcile will tear us down — exit
                                    // early so we don't fire one extra
                                    // probe after pause.
                                    info!(monitor = %monitor_id, "monitor inactive — probe task exiting");
                                    return;
                                }
                                run_once(&probes, &fresh, &last_status_in_task,
                                         &hb_tx, notifier.as_ref(), &pool_in_task).await;
                                interval = Duration::from_secs(fresh.interval_seconds as u64);
                            }
                            Err(rampart_db::DbError::NotFound) => {
                                info!(monitor = %monitor_id, "monitor deleted — probe task exiting");
                                return;
                            }
                            Err(e) => {
                                // Transient DB blip — log and keep the
                                // old config + interval rather than
                                // killing the loop.
                                warn!(monitor = %monitor_id, error = %e, "monitor refresh failed; reusing prior snapshot");
                                run_once(&probes, &monitor, &last_status_in_task,
                                         &hb_tx, notifier.as_ref(), &pool_in_task).await;
                            }
                        }
                    }
                }
            }
        });

        self.tasks.write().await.insert(
            monitor_id,
            MonitorTask {
                handle,
                cancel,
                last_status,
            },
        );
    }

    async fn stop_task(&self, id: MonitorId) {
        let task = self.tasks.write().await.remove(&id);
        if let Some(t) = task {
            t.cancel.notify_one();
            // Drop the JoinHandle without awaiting — the task will wake on
            // the next tick and notice it was cancelled. We deliberately
            // don't .abort(): risks killing the writer mid-flush.
            drop(t.handle);
        }
    }
}

/// Run one probe iteration, set the important flag if the status flipped,
/// push the heartbeat onto the writer channel, optionally emit a
/// notifier event, and update DB state.
async fn run_once(
    probes: &Probes,
    monitor: &Monitor,
    last_status: &Arc<RwLock<MonitorStatus>>,
    hb_tx: &mpsc::Sender<Heartbeat>,
    notifier: Option<&NotifierHandle>,
    pool: &DbPool,
) {
    // Maintenance suppression. If the monitor is inside an active
    // window we skip the probe entirely and emit a synthetic
    // Maintenance heartbeat — keeps the timeline contiguous for the
    // dashboard but doesn't fire notifications. We swallow DB errors
    // here so a transient blip doesn't take down the probe loop;
    // worst case we run an unneeded probe.
    let in_maintenance = rampart_db::maintenance::is_in_active_window(pool, monitor.id)
        .await
        .unwrap_or(false);

    let mut hb = if in_maintenance {
        maintenance_heartbeat(monitor)
    } else if monitor.kind == MonitorKind::Push {
        // Push monitors are inverted — the external job calls us, not
        // the other way around. The scheduler's job here is just to
        // check if a push has landed inside the expected interval.
        // Probe crate doesn't touch the DB (layer rule), so we
        // synthesize the heartbeat here.
        push_heartbeat(monitor, pool).await
    } else if let Some(pid) = monitor.proxy_id {
        // HTTP-family kinds + a configured proxy route through the
        // dedicated HttpProbe::run_with_proxy path. Other kinds with a
        // dangling proxy_id (e.g. a TCP probe) silently ignore it.
        match rampart_db::proxies::get(pool, pid).await {
            Ok(proxy) if proxy.active
                && matches!(monitor.kind,
                    MonitorKind::Http | MonitorKind::Keyword | MonitorKind::JsonQuery) => {
                probes.http_with_proxy(monitor, &proxy).await
            }
            _ => probes.run(monitor).await,
        }
    } else {
        probes.run(monitor).await
    };

    // Mark this heartbeat as important if it flipped the status. Don't
    // fire an event on the very first observation (prev == Pending) for
    // a monitor that's already up — that's just initialisation, not a
    // user-visible flip. Also suppress events for any flip *into* or
    // *out of* Maintenance: those are admin-driven, not real outages.
    let prev = *last_status.read().await;
    let flipped = prev != hb.status;
    if flipped {
        hb.important = true;
        *last_status.write().await = hb.status;

        let user_visible_flip = !(prev == MonitorStatus::Pending && hb.status == MonitorStatus::Up)
            && hb.status != MonitorStatus::Maintenance
            && prev      != MonitorStatus::Maintenance;
        if user_visible_flip {
            if let Some(n) = notifier {
                n.notify(Event {
                    kind: EventKind::StatusFlip,
                    monitor: monitor.clone(),
                    heartbeat: hb.clone(),
                    prev_status: Some(prev),
                });
            }
        }
    }

    if hb_tx.send(hb).await.is_err() {
        warn!(monitor = %monitor.id, "heartbeat channel closed; dropping");
    }

    // Refresh TLS cert snapshot for HTTPS HTTP-family monitors. Rate-limited
    // to once per hour per monitor — re-running on every tick would dwarf
    // the probe itself for short intervals.
    if matches!(monitor.kind, MonitorKind::Http | MonitorKind::Keyword | MonitorKind::JsonQuery) {
        if let Some(url) = monitor.url.as_deref() {
            if url.starts_with("https://") {
                let stale = monitor.cert_checked_at
                    .map(|t| (time::OffsetDateTime::now_utc() - t).whole_seconds() >= 3600)
                    .unwrap_or(true);
                if stale {
                    let pool = pool.clone();
                    let id = monitor.id;
                    let url_owned = url.to_string();
                    let to = Duration::from_secs(monitor.timeout_seconds.max(10) as u64);
                    tokio::spawn(async move {
                        if let Some((host, port)) = parse_https(&url_owned) {
                            if let Ok(snap) =
                                rampart_checker::tls::fetch_cert(&host, port, to).await
                            {
                                let _ = rampart_db::monitors::set_cert_info(
                                    &pool, id, snap.days_left, &snap.subject,
                                )
                                .await;
                            }
                        }
                    });
                }
            }
        }
    }
}

fn parse_https(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("https://")?;
    let host_part = rest.split('/').next()?;
    // strip optional userinfo + handle [v6] later — fine for current scope
    let (host, port) = match host_part.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None         => (host_part.to_string(), 443u16),
    };
    Some((host, port))
}

/// Drains the heartbeat channel, batches by size or time, flushes.
/// Successfully-persisted heartbeats are also fanned out to live
/// subscribers via `broadcast` — never before the flush so the API can
/// safely assume any streamed row is queryable.
async fn writer_loop(
    pool: DbPool,
    mut rx: mpsc::Receiver<Heartbeat>,
    bcast: broadcast::Sender<Heartbeat>,
) {
    let mut buffer: Vec<Heartbeat> = Vec::with_capacity(BATCH_SIZE);

    loop {
        // Wait for at least one heartbeat. If the channel closes, drain
        // remaining buffer and exit.
        let first = match rx.recv().await {
            Some(h) => h,
            None => {
                if !buffer.is_empty() {
                    flush(&pool, &buffer).await;
                }
                info!("heartbeat writer shutting down");
                return;
            }
        };
        buffer.push(first);

        // Now try to fill the batch with anything immediately available,
        // up to BATCH_SIZE total, or wait BATCH_FLUSH_INTERVAL.
        let flush_at = tokio::time::Instant::now() + BATCH_FLUSH_INTERVAL;
        while buffer.len() < BATCH_SIZE {
            tokio::select! {
                hb = rx.recv() => match hb {
                    Some(h) => buffer.push(h),
                    None    => break,
                },
                _ = tokio::time::sleep_until(flush_at) => break,
            }
        }

        if flush(&pool, &buffer).await {
            // send() returns Err when there are zero subscribers — that
            // is normal (nobody watching) so ignore. Lagged consumers see
            // RecvError::Lagged on their own receiver instead.
            for hb in &buffer {
                let _ = bcast.send(hb.clone());
            }
        }
        buffer.clear();

        // Also bounce the monitor's current_status if any heartbeat in
        // the batch was important. Done after the heartbeat insert so a
        // dashboard reader never sees current_status ahead of the
        // heartbeat that backs it.
        // (We don't keep the original batch around after clear(), so
        // tracking-via-side-channel would be cleaner — left as a TODO
        // until the API needs it for live updates.)
    }
}

/// Returns `true` if the batch persisted — callers gate the live-stream
/// fan-out on this so subscribers never receive rows that aren't on disk.
async fn flush(pool: &DbPool, batch: &[Heartbeat]) -> bool {
    if let Err(e) = rampart_db::heartbeats::insert_many(pool, batch).await {
        // Don't crash the writer on a DB blip — log loudly, keep going.
        // The probe loop will produce more heartbeats on the next tick.
        error!(error = %e, batch = batch.len(), "heartbeat flush failed");
        return false;
    }
    true
}

/// Synthesize a Maintenance heartbeat — keeps the timeline contiguous
/// while a monitor sits inside an active maintenance window so the
/// dashboard's uptime strip doesn't develop gaps.
fn maintenance_heartbeat(monitor: &Monitor) -> Heartbeat {
    use time::OffsetDateTime;
    Heartbeat {
        monitor_id:  monitor.id,
        ts:          OffsetDateTime::now_utc(),
        status:      MonitorStatus::Maintenance,
        latency_ms:  None,
        status_code: None,
        msg:         Some("in maintenance".into()),
        retries:     0,
        important:   false,
    }
}

/// Synthesize a heartbeat for a Push monitor by reading its last_push_at
/// from the database. Up if a push landed within the last `interval`
/// seconds (with a small grace), Down otherwise. The grace covers the
/// case where the external job is on its own cron and lands a moment
/// after our tick — without it, perfectly-on-time pushes would flap.
async fn push_heartbeat(monitor: &Monitor, pool: &DbPool) -> Heartbeat {
    use time::OffsetDateTime;

    let now = OffsetDateTime::now_utc();
    let last = rampart_db::monitors::fetch_last_push_at(pool, monitor.id)
        .await
        .ok()
        .flatten();

    let (status, msg) = match last {
        None => (
            MonitorStatus::Down,
            Some("no push received yet".into()),
        ),
        Some(ts) => {
            let elapsed = (now - ts).whole_seconds();
            // 10s grace, scaled by interval if very small intervals are set.
            let grace = (monitor.interval_seconds / 10).max(10) as i64;
            if elapsed <= (monitor.interval_seconds as i64) + grace {
                (MonitorStatus::Up, Some(format!("push {}s ago", elapsed)))
            } else {
                (
                    MonitorStatus::Down,
                    Some(format!("no push for {}s", elapsed)),
                )
            }
        }
    };

    Heartbeat {
        monitor_id:  monitor.id,
        ts:          now,
        status,
        latency_ms:  None,
        status_code: None,
        msg,
        retries:     0,
        important:   false,
    }
}

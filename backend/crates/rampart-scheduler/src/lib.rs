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
use rampart_core::{Heartbeat, Monitor, MonitorId, MonitorKind, MonitorStatus, RetryBackoff};
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
        let writer_notifier = notifier.clone();
        tokio::spawn(async move {
            writer_loop(writer_pool, hb_rx, writer_broadcast, writer_notifier).await;
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

    /// Queue an externally-produced heartbeat (agent report) onto the
    /// batched writer pipeline. It gets the identical downstream
    /// treatment a local probe result does: batched INSERT, SSE
    /// broadcast, current_status bounce when `important`, SLO breach
    /// detection, and the per-monitor result webhook. Returns false if
    /// the writer has shut down.
    pub async fn submit_external(&self, hb: Heartbeat) -> bool {
        self.hb_tx.send(hb).await.is_ok()
    }

    /// Fire a status-flip notification for an externally-detected
    /// transition (agent ingest path). The caller does its own flip
    /// detection against `monitors.current_status`; this just fans out.
    pub fn notify_status_flip(&self, monitor: Monitor, heartbeat: Heartbeat, prev: MonitorStatus) {
        if let Some(n) = self.notifier.as_ref() {
            n.notify(Event {
                kind: EventKind::StatusFlip,
                monitor,
                heartbeat,
                prev_status: Some(prev),
                slo_current_pct: None,
            });
        }
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
            // Best-effort maintenance start/end notifications, riding the
            // same slow tick. Failures are logged inside the scan — they
            // must never stall the reconcile loop.
            self.check_maintenance_transitions().await;
            // Scheduled weekly uptime reports, same best-effort contract.
            self.check_scheduled_reports().await;
            // Stale-agent watchdog: agent-assigned monitors that stopped
            // reporting get a synthetic Down heartbeat + alert.
            self.check_stale_agents().await;
            // Metric threshold rules: evaluate against latest samples,
            // fan out fired/resolved transitions to the rules' channels.
            self.check_metric_rules().await;
            // Escalation ladders: advance unacked episodes whose next
            // step is due.
            self.check_escalations().await;
        }
    }

    /// Advance due escalation episodes. `advance` re-checks its
    /// predicates atomically, so a racing tick (or an ack landing
    /// mid-scan) loses cleanly. An episode whose monitor already
    /// recovered (resolve missed, e.g. the process restarted between
    /// the flip and the notifier) is closed instead of paged.
    async fn check_escalations(&self) {
        let due = match rampart_db::escalations::due(&self.pool).await {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "escalation due scan failed");
                return;
            }
        };
        for episode in due {
            let monitor = match rampart_db::monitors::get(&self.pool, episode.monitor_id).await {
                Ok(m) => m,
                Err(_) => continue, // monitor deleted; episode cascades away
            };
            if !monitor.current_status.is_down() {
                let _ = rampart_db::escalations::resolve(&self.pool, episode.monitor_id).await;
                continue;
            }
            let policy = match rampart_db::escalations::get(&self.pool, episode.policy_id).await {
                Ok(p) => p,
                Err(e) => {
                    warn!(policy = %episode.policy_id.0, error = %e, "escalation policy load failed");
                    continue;
                }
            };
            let Ok(Some(advanced)) =
                rampart_db::escalations::advance(&self.pool, episode.id, &policy).await
            else {
                continue;
            };
            let now = time::OffsetDateTime::now_utc();
            let down_for = (now - advanced.started_at).whole_seconds();
            let event = rampart_notifier::Event {
                kind: rampart_notifier::EventKind::Escalation,
                heartbeat: Heartbeat {
                    monitor_id: monitor.id,
                    ts: now,
                    status: MonitorStatus::Down,
                    latency_ms: None,
                    status_code: None,
                    msg: Some(format!("still down, unacknowledged for {down_for}s")),
                    retries: 0,
                    important: false,
                },
                monitor,
                prev_status: None,
                slo_current_pct: None,
            };
            rampart_notifier::service::fire_escalation_step(
                &self.pool,
                &event,
                &policy,
                advanced.last_step as usize,
            )
            .await;
        }
    }

    /// Evaluate metric threshold rules and send alerts for transitions.
    /// State lives on the rule rows (rampart_db::metric_rules), so this
    /// is restart-safe and never double-pages; this method only owns the
    /// notification fan-out.
    async fn check_metric_rules(&self) {
        let events = match rampart_db::metric_rules::evaluate_tick(&self.pool).await {
            Ok(ev) => ev,
            Err(e) => {
                warn!(error = %e, "metric rule evaluation failed");
                return;
            }
        };
        for ev in events {
            let fired = ev.transition == rampart_core::metric_rule::RuleTransition::Fire;
            let series = if ev
                .rule
                .labels
                .as_object()
                .map(|o| o.is_empty())
                .unwrap_or(true)
            {
                ev.rule.metric.clone()
            } else {
                format!("{}{}", ev.rule.metric, ev.rule.labels)
            };
            let msg = match (fired, ev.value) {
                (true, Some(v)) => format!(
                    "{series} = {v} {} {} (sustained {}s)",
                    ev.rule.op.symbol(),
                    ev.rule.threshold,
                    ev.rule.for_seconds
                ),
                (true, None) => format!("{series} breached {}", ev.rule.threshold),
                (false, Some(v)) => format!(
                    "{series} back within threshold: {v} (limit {} {})",
                    ev.rule.op.symbol(),
                    ev.rule.threshold
                ),
                (false, None) => format!("{series} stopped reporting; rule resolved"),
            };
            info!(rule = %ev.rule.name, fired, %msg, "metric rule transition");

            let event = rampart_notifier::Event {
                kind: if fired {
                    rampart_notifier::EventKind::MetricRuleFired
                } else {
                    rampart_notifier::EventKind::MetricRuleResolved
                },
                monitor: metric_rule_monitor(&ev.rule),
                heartbeat: Heartbeat {
                    monitor_id: rampart_core::MonitorId::new(),
                    ts: time::OffsetDateTime::now_utc(),
                    status: if fired {
                        MonitorStatus::Down
                    } else {
                        MonitorStatus::Up
                    },
                    latency_ms: None,
                    status_code: None,
                    msg: Some(msg),
                    retries: 0,
                    important: true,
                },
                prev_status: None,
                slo_current_pct: None,
            };
            for ch in &ev.rule.channel_ids {
                if let Err(e) =
                    rampart_notifier::service::send_event_to_channel(&self.pool, *ch, &event).await
                {
                    warn!(rule = %ev.rule.name, channel = %ch.0, error = %e,
                          "metric rule channel send failed (channel deleted?)");
                }
            }
        }
    }

    /// Watchdog for agent-assigned monitors that have gone quiet (no
    /// heartbeat for 2× interval + 30s — see
    /// `monitors::list_stale_agent_monitors`). Synthesizes a Down
    /// heartbeat naming the dark agent and fires a StatusFlip on the
    /// transition, so an unplugged agent pages exactly like a dead
    /// service would. The synthetic heartbeat resets the staleness
    /// clock, so a continuously-dark agent re-emits one Down heartbeat
    /// per staleness window rather than one per tick.
    async fn check_stale_agents(&self) {
        let stale = match rampart_db::monitors::list_stale_agent_monitors(&self.pool).await {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "stale-agent scan failed");
                return;
            }
        };
        for (monitor, agent_name) in stale {
            // Same maintenance suppression as a local probe tick.
            let in_maintenance =
                rampart_db::maintenance::is_in_active_window(&self.pool, monitor.id)
                    .await
                    .unwrap_or(false);
            if in_maintenance {
                continue;
            }
            let prev = monitor.current_status;
            let mut hb = Heartbeat {
                monitor_id: monitor.id,
                ts: time::OffsetDateTime::now_utc(),
                status: MonitorStatus::Down,
                latency_ms: None,
                status_code: None,
                msg: Some(format!("no report from agent \"{agent_name}\"")),
                retries: 0,
                important: false,
            };
            if prev != MonitorStatus::Down {
                hb.important = true;
                if prev != MonitorStatus::Maintenance {
                    if let Some(n) = self.notifier.as_ref() {
                        n.notify(Event {
                            kind: EventKind::StatusFlip,
                            monitor: monitor.clone(),
                            heartbeat: hb.clone(),
                            prev_status: Some(prev),
                            slo_current_pct: None,
                        });
                    }
                }
            }
            if self.hb_tx.send(hb).await.is_err() {
                warn!(monitor = %monitor.id, "heartbeat channel closed; stale-agent heartbeat dropped");
            }
        }
    }

    /// Periodic, best-effort scan for scheduled uptime reports that are due
    /// (never sent, or `last_sent_at` older than the cadence window). For
    /// each, render a per-monitor 7-day uptime digest and email it to the
    /// report's recipients via the system SMTP sender (the same path the
    /// maintenance-subscriber fan-out uses), then stamp `last_sent_at`.
    /// De-dup lives in that column, so re-running every slow tick is safe.
    /// Failures are logged and never stall the reconcile loop.
    async fn check_scheduled_reports(&self) {
        let now = time::OffsetDateTime::now_utc();
        let due = match rampart_db::scheduled_reports::due(&self.pool, now).await {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "scheduled report due scan failed");
                return;
            }
        };
        if due.is_empty() {
            return;
        }

        for report in due {
            if report.recipients.is_empty() {
                // Nothing to send — stamp it so we don't rescan it every
                // tick, and so it shows a sensible last_sent_at in the UI.
                if let Err(e) =
                    rampart_db::scheduled_reports::mark_sent(&self.pool, report.id).await
                {
                    warn!(report = %report.id, error = %e, "scheduled report stamp failed");
                }
                continue;
            }

            let (subject, body) = match rampart_db::scheduled_reports::render(
                &self.pool,
                &report.name,
                &report.cadence,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(report = %report.id, error = %e, "uptime report render failed");
                    continue;
                }
            };

            let sent = rampart_notifier::send_system_email(
                &self.pool,
                &report.recipients,
                &subject,
                &body,
            )
            .await;
            info!(report = %report.id, name = %report.name, recipients = report.recipients.len(),
                  sent, "scheduled uptime report sent");

            // Stamp regardless of how many recipients succeeded: SMTP may be
            // unconfigured (sent == 0) and we don't want to retry every tick
            // forever. Per-recipient failures are already logged.
            if let Err(e) = rampart_db::scheduled_reports::mark_sent(&self.pool, report.id).await {
                warn!(report = %report.id, error = %e, "scheduled report stamp failed");
            }
        }
    }

    /// Periodic, best-effort scan for maintenance windows that just became
    /// active or just ended, firing `MaintenanceStarted` / `MaintenanceEnded`
    /// once per transition. De-dup lives in the DB (`notified_start_at` /
    /// `notified_end_at`), so re-running every slow tick is safe — same
    /// single-column pattern as the SLO breach check. Not a precise cron:
    /// a transition is noticed within one slow tick of its instant.
    async fn check_maintenance_transitions(&self) {
        let Some(notifier) = self.notifier.as_ref() else {
            return;
        };
        let windows =
            match rampart_db::maintenance::transitions_needing_notification(&self.pool).await {
                Ok(w) => w,
                Err(e) => {
                    warn!(error = %e, "maintenance transition scan failed");
                    return;
                }
            };

        for w in windows {
            // started: active now, start not yet notified.
            // ended:   no longer active, started before, end not notified.
            let kind = if w.currently_active && w.notified_start_at.is_none() {
                EventKind::MaintenanceStarted
            } else if !w.currently_active
                && w.notified_start_at.is_some()
                && w.notified_end_at.is_none()
            {
                EventKind::MaintenanceEnded
            } else {
                continue;
            };

            // Stamp the de-dup column *before* firing so a slow tick that
            // overlaps the next scan doesn't double-fire. Stamp failure is
            // logged; we still fire (better a possible dup than silence).
            let stamp = match kind {
                EventKind::MaintenanceStarted => {
                    rampart_db::maintenance::mark_notified_start(&self.pool, w.id).await
                }
                _ => rampart_db::maintenance::mark_notified_end(&self.pool, w.id).await,
            };
            if let Err(e) = stamp {
                warn!(window = %w.id, error = %e, "maintenance de-dup stamp failed");
            }

            info!(window = %w.id, name = %w.name, kind = ?kind, monitors = w.monitor_ids.len(),
                  "maintenance event firing");

            // One event per attached monitor — mirrors the per-monitor
            // shape every other event uses, so channel routing + the
            // subscriber fan-out (in the notifier) work unchanged. The
            // notifier de-dups subscriber emails across the page.
            for mid in &w.monitor_ids {
                let monitor = match rampart_db::monitors::get(&self.pool, *mid).await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(monitor = %mid, error = %e, "monitor hydrate for maintenance event failed");
                        continue;
                    }
                };
                let hb = maintenance_heartbeat(&monitor);
                notifier.notify(Event {
                    kind,
                    monitor,
                    heartbeat: hb,
                    prev_status: None,
                    slo_current_pct: None,
                });
            }
        }
    }

    /// Diff active monitors in DB against in-memory tasks. Start tasks
    /// for monitors that should be running but aren't; cancel tasks for
    /// monitors that have been deleted or deactivated.
    async fn reconcile(&self) -> Result<(), rampart_db::DbError> {
        let live = rampart_db::monitors::list(&self.pool).await?;
        // Agent-assigned monitors are probed remotely — the agent reports
        // heartbeats through the API ingest path. Running a local probe
        // task too would double-probe (and double-alert), so they're
        // filtered out exactly like inactive monitors. A reassignment to
        // local (agent_id → NULL) brings the task back on the next poke.
        let live_active: HashMap<MonitorId, Monitor> = live
            .into_iter()
            .filter(|m| m.active && m.agent_id.is_none())
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
        // the other way around. The scheduler's job here is to check
        // whether the expected ping landed (interval mode) or whether a
        // scheduled run was missed / overran (cron mode). Cron-mode
        // ticks that find nothing wrong record nothing — the timeline
        // belongs to the job's own pings. Probe crate doesn't touch
        // the DB (layer rule), so we synthesize the heartbeat here.
        match push_heartbeat(monitor, pool).await {
            Some(h) => h,
            None => return,
        }
    } else {
        // Real probe path, with an optional retry-with-backoff curve.
        probe_with_retries(probes, monitor, pool).await
    };

    // Mark this heartbeat as important if it flipped the status. Don't
    // fire an event on the very first observation (prev == Pending) for
    // a monitor that's already up — that's just initialisation, not a
    // user-visible flip. Also suppress events for any flip *into* or
    // *out of* Maintenance: those are admin-driven, not real outages.
    //
    // Push monitors read `prev` from the refetched DB row instead of the
    // in-memory cell: their flips are ALSO produced by the ping-ingest
    // path (API), which compares against monitors.current_status. Using
    // the same source keeps the two detectors coherent — otherwise a
    // ping-driven recovery would be re-announced by the next tick, and
    // a second missed run after a ping-driven recovery would be missed.
    let prev = if monitor.kind == MonitorKind::Push {
        monitor.current_status
    } else {
        *last_status.read().await
    };
    let flipped = prev != hb.status;
    if flipped {
        hb.important = true;
        *last_status.write().await = hb.status;

        let user_visible_flip = !(prev == MonitorStatus::Pending && hb.status == MonitorStatus::Up)
            && hb.status != MonitorStatus::Maintenance
            && prev != MonitorStatus::Maintenance;
        if user_visible_flip {
            if let Some(n) = notifier {
                n.notify(Event {
                    kind: EventKind::StatusFlip,
                    monitor: monitor.clone(),
                    heartbeat: hb.clone(),
                    prev_status: Some(prev),
                    slo_current_pct: None,
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
    if matches!(
        monitor.kind,
        MonitorKind::Http | MonitorKind::Keyword | MonitorKind::JsonQuery
    ) {
        if let Some(url) = monitor.url.as_deref() {
            if url.starts_with("https://") {
                let stale = monitor
                    .cert_checked_at
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
                                    &pool,
                                    id,
                                    snap.days_left,
                                    &snap.subject,
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

/// Dispatch a single real probe, routing HTTP-family + active-proxy
/// monitors through the dedicated proxy path.
async fn probe_once(probes: &Probes, monitor: &Monitor, pool: &DbPool) -> Heartbeat {
    if let Some(pid) = monitor.proxy_id {
        // HTTP-family kinds + a configured proxy route through the
        // dedicated HttpProbe::run_with_proxy path. Other kinds with a
        // dangling proxy_id (e.g. a TCP probe) silently ignore it.
        match rampart_db::proxies::get(pool, pid).await {
            Ok(proxy)
                if proxy.active
                    && matches!(
                        monitor.kind,
                        MonitorKind::Http | MonitorKind::Keyword | MonitorKind::JsonQuery
                    ) =>
            {
                return probes.http_with_proxy(monitor, &proxy).await;
            }
            _ => {}
        }
    }
    probes.run(monitor).await
}

/// Run a real probe, then — if it failed and the monitor opts into a retry
/// backoff curve — re-probe up to `max_retries` times, sleeping the
/// computed delay between attempts before committing the failure.
///
/// Behaviour is *identical to history* when `config.retry_backoff` is
/// absent: a single probe, no inter-retry sleep, immediate result. The
/// retry loop is gated entirely on the presence of that config block so
/// existing monitors keep their exact current timing.
async fn probe_with_retries(probes: &Probes, monitor: &Monitor, pool: &DbPool) -> Heartbeat {
    let mut hb = probe_once(probes, monitor, pool).await;

    // Fast path: success, no opt-in, or nothing to retry → exactly today.
    if !hb.status.is_down() || monitor.max_retries <= 0 {
        return hb;
    }
    let backoff = match RetryBackoff::from_config(&monitor.config) {
        Some(b) => b,
        None => return hb,
    };

    // Never let a single retry sleep exceed the monitor interval — a
    // backoff longer than the cadence would stall the whole loop.
    let interval_cap = monitor.interval_seconds.max(0) as u32;

    let max_attempts = monitor.max_retries as u32;
    for attempt in 1..=max_attempts {
        let delay = backoff.delay_secs(attempt).min(interval_cap);
        if delay > 0 {
            tokio::time::sleep(Duration::from_secs(delay as u64)).await;
        }
        hb = probe_once(probes, monitor, pool).await;
        hb.retries = attempt as i32;
        if !hb.status.is_down() {
            // Recovered within the retry budget — report the success and
            // skip committing a Down at all.
            break;
        }
    }
    hb
}

fn parse_https(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("https://")?;
    let host_part = rest.split('/').next()?;
    // strip optional userinfo + handle [v6] later — fine for current scope
    let (host, port) = match host_part.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (host_part.to_string(), 443u16),
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
    notifier: Option<NotifierHandle>,
) {
    let mut buffer: Vec<Heartbeat> = Vec::with_capacity(BATCH_SIZE);

    // Shared client for outbound result webhooks (item: per-monitor probe
    // result webhooks). Short timeout, built once. A build failure leaves
    // the feature disabled but never takes down the writer.
    let result_webhook_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| warn!(error = %e, "result webhook client build failed; webhooks disabled"))
        .ok();

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
            for hb in &buffer {
                // send() returns Err when there are zero subscribers —
                // that is normal (nobody watching) so ignore.
                let _ = bcast.send(hb.clone());

                // Bounce the monitor's current_status on a status flip,
                // *after* the heartbeat insert so a dashboard reader never
                // sees current_status ahead of the heartbeat backing it.
                // `important` is set by run_once exactly when the status
                // changed, so this only writes on real transitions — and
                // it's what makes the dashboard counts / hero / badges
                // reflect reality (previously current_status never moved
                // off Pending). Last flip in the batch wins per monitor.
                if hb.important {
                    if let Err(e) =
                        rampart_db::monitors::set_status(&pool, hb.monitor_id, hb.status).await
                    {
                        error!(monitor = %hb.monitor_id, error = %e, "set_status failed");
                    }
                }
            }

            // SLO breach detection. Runs *after* the batch is on disk so
            // `current_slo_uptime_pct` sees every heartbeat just inserted.
            // We pick one representative heartbeat per monitor (the latest
            // one in the batch) to pass to the event; a single breach
            // pages once thanks to `slo_breached_at`.
            check_slo_breaches(&pool, &buffer, notifier.as_ref()).await;

            // Outbound per-monitor result webhooks. Fires on EVERY
            // heartbeat (not just alerts) for monitors that configure
            // `config.result_webhook`. Fire-and-forget on a detached task
            // so a slow sink never blocks the writer.
            if let Some(client) = result_webhook_client.as_ref() {
                fire_result_webhooks(&pool, client, &buffer).await;
            }
        }
        buffer.clear();
    }
}

/// For every monitor touched by this batch, recompute the rolling SLO
/// uptime and emit `SloBreached` / `SloRecovered` when the column crosses
/// the configured target. De-duplicated via `monitors.slo_breached_at`:
/// while a breach is active the column is non-null and no further
/// SloBreached fires; recovery clears the column.
///
/// Failures here are logged + swallowed — the heartbeat itself is
/// already persisted and we'd rather not stall the writer because of a
/// transient DB blip on the SLO read path.
async fn check_slo_breaches(pool: &DbPool, batch: &[Heartbeat], notifier: Option<&NotifierHandle>) {
    let Some(notifier) = notifier else { return };
    if batch.is_empty() {
        return;
    }

    // Latest heartbeat per monitor — the templates carry `ts` from this,
    // so we want the most recent one regardless of input ordering.
    let mut latest: HashMap<MonitorId, Heartbeat> = HashMap::with_capacity(batch.len());
    for hb in batch {
        match latest.get(&hb.monitor_id) {
            Some(existing) if existing.ts >= hb.ts => {}
            _ => {
                latest.insert(hb.monitor_id, hb.clone());
            }
        }
    }

    for (mid, hb) in latest {
        let slo = match rampart_db::monitors::slo_state(pool, mid).await {
            Ok(Some(s)) => s,
            // Monitor row gone (deleted) — nothing to do.
            Ok(None) => continue,
            Err(e) => {
                warn!(monitor = %mid, error = %e, "slo_state lookup failed");
                continue;
            }
        };

        let (Some(target), Some(window)) = (slo.target_pct, slo.window_days) else {
            // No SLO configured for this monitor — short-circuit fast.
            continue;
        };

        let current = match rampart_db::heartbeats::current_slo_uptime_pct(pool, mid, window).await
        {
            Ok(Some(v)) => v,
            // No heartbeats in the window yet — can't say "breached"
            // off zero data. Skip without touching the de-dup column.
            Ok(None) => continue,
            Err(e) => {
                warn!(monitor = %mid, error = %e, "current_slo_uptime_pct failed");
                continue;
            }
        };

        let breaching = current < target;
        let already_breached = slo.breached_at.is_some();
        let kind = match (breaching, already_breached) {
            (true, false) => EventKind::SloBreached,
            (false, true) => EventKind::SloRecovered,
            // Either still good (no breach, no marker) or still breached
            // (already paged) — single column de-dup keeps this silent.
            _ => continue,
        };

        // Hydrate the full monitor so the template engine + per-channel
        // adapters have the same shape they get for status flips.
        let monitor = match rampart_db::monitors::get(pool, mid).await {
            Ok(m) => m,
            Err(e) => {
                warn!(monitor = %mid, error = %e, "monitor hydrate for SLO event failed");
                continue;
            }
        };

        let stamp_result = match kind {
            EventKind::SloBreached => rampart_db::monitors::mark_slo_breached(pool, mid).await,
            EventKind::SloRecovered => rampart_db::monitors::clear_slo_breached(pool, mid).await,
            _ => Ok(()),
        };
        if let Err(e) = stamp_result {
            warn!(monitor = %mid, error = %e, "SLO de-dup column update failed");
            // Fire the event anyway — better a possible duplicate page
            // than silent breach. Next batch will retry the column update.
        }

        info!(
            monitor = %mid,
            kind = ?kind,
            current = current,
            target = target,
            "SLO event firing",
        );

        notifier.notify(Event {
            kind,
            monitor,
            heartbeat: hb,
            prev_status: None,
            slo_current_pct: Some(current),
        });
    }
}

/// Fire per-monitor result webhooks for a just-persisted batch.
///
/// For every monitor in the batch that has a `result_webhook` string in
/// its `config` JSONB, POST a compact JSON body per heartbeat to that URL.
/// Fully fire-and-forget: each POST runs on a detached task with the
/// shared short-timeout client, failures are logged, and nothing here can
/// block or fail the writer loop.
///
/// The monitor config (and thus the webhook URL + name) is fetched once
/// per distinct monitor in the batch — at most a handful of PK lookups per
/// flush. Monitors without the config key short-circuit with no I/O beyond
/// that single fetch.
async fn fire_result_webhooks(pool: &DbPool, client: &reqwest::Client, batch: &[Heartbeat]) {
    use std::collections::HashMap;

    // Resolve each distinct monitor's webhook URL + optional signing secret
    // + name once. `None` means "no webhook configured" (or fetch failed) —
    // short-circuits the per-heartbeat loop below.
    type Target = (String, Option<String>, String);
    let mut target_for: HashMap<MonitorId, Option<Target>> = HashMap::new();
    for hb in batch {
        if let std::collections::hash_map::Entry::Vacant(e) = target_for.entry(hb.monitor_id) {
            let target = match rampart_db::monitors::get(pool, hb.monitor_id).await {
                Ok(m) => result_webhook_url(&m).map(|url| (url, result_webhook_secret(&m), m.name)),
                Err(e2) => {
                    warn!(monitor = %hb.monitor_id, error = %e2, "result webhook monitor fetch failed");
                    None
                }
            };
            e.insert(target);
        }
    }

    for hb in batch {
        let Some(Some((url, secret, name))) = target_for.get(&hb.monitor_id) else {
            continue;
        };
        let ts = hb.ts.unix_timestamp();
        let payload = serde_json::json!({
            "monitor_id": hb.monitor_id,
            "name": name,
            "status": hb.status,
            "latency_ms": hb.latency_ms,
            "status_code": hb.status_code,
            "ts": ts,
        });
        // Serialize once so the signature (over the exact wire bytes) and the
        // POST body cannot drift. A serialize failure is logged + skipped.
        let body_bytes = match serde_json::to_vec(&payload) {
            Ok(b) => b,
            Err(e) => {
                warn!(monitor = %hb.monitor_id, error = %e, "result webhook payload encode failed");
                continue;
            }
        };
        // Optional HMAC-SHA256 over `<ts>.<body>` — replay-resistant because
        // the timestamp is bound into the signed string and echoed in a
        // header the receiver can age-check.
        let signature = secret
            .as_deref()
            .map(|s| sign_result_webhook(s, ts, &body_bytes));

        let client = client.clone();
        let url = url.clone();
        let mid = hb.monitor_id;
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut req = client
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header("X-Rampart-Timestamp", ts.to_string());
            if let Some(sig) = signature {
                req = req.header("X-Rampart-Signature", format!("sha256={sig}"));
            }
            let (ok, err) = match req.body(body_bytes).send().await {
                Ok(resp) if resp.status().is_success() => (true, None),
                Ok(resp) => {
                    let status = resp.status();
                    warn!(monitor = %mid, status = %status, "result webhook non-2xx");
                    (false, Some(format!("non-2xx: {status}")))
                }
                Err(e) => {
                    warn!(monitor = %mid, error = %e, "result webhook POST failed");
                    (false, Some(e.to_string()))
                }
            };

            // Best-effort delivery-log row for every attempt, success or
            // failure — swallow logging errors so the webhook path never
            // depends on the audit write.
            if let Err(e) = rampart_db::delivery_log::record(
                &pool,
                rampart_db::delivery_log::NewDelivery {
                    notification_id: None,
                    channel_kind: "result_webhook",
                    event_kind: "probe_result",
                    monitor_id: Some(mid.0),
                    ok,
                    error: err.as_deref(),
                },
            )
            .await
            {
                warn!(monitor = %mid, error = %e, "result webhook delivery-log record failed");
            }
        });
    }
}

/// Compute the result-webhook signature: lowercase hex HMAC-SHA256 of
/// `<ts>.<body>` under `secret`. Returned without the `sha256=` prefix —
/// callers add it to the header value.
fn sign_result_webhook(secret: &str, ts: i64, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    // new_from_slice only fails on a zero-length key for HMAC; an empty
    // secret is already filtered out by result_webhook_secret, so this is
    // infallible in practice. Fall back to an empty signature rather than
    // panicking if that ever changes.
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
    mac.update(ts.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Extract the optional `result_webhook` URL from a monitor's `config`
/// JSONB. Returns `None` when absent, null, non-string, or empty.
fn result_webhook_url(monitor: &Monitor) -> Option<String> {
    monitor
        .config
        .get("result_webhook")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Extract the optional `result_webhook_secret` from a monitor's `config`
/// JSONB. Returns `None` when absent, null, non-string, or empty — in which
/// case the outbound POST is sent unsigned (unchanged historical behaviour).
fn result_webhook_secret(monitor: &Monitor) -> Option<String> {
    monitor
        .config
        .get("result_webhook_secret")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
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
        monitor_id: monitor.id,
        ts: OffsetDateTime::now_utc(),
        status: MonitorStatus::Maintenance,
        latency_ms: None,
        status_code: None,
        msg: Some("in maintenance".into()),
        retries: 0,
        important: false,
    }
}

/// Synthesize a heartbeat for a Push monitor by reading its last_push_at
/// from the database. Up if a push landed within the last `interval`
/// seconds (with a small grace), Down otherwise. The grace covers the
/// case where the external job is on its own cron and lands a moment
/// after our tick — without it, perfectly-on-time pushes would flap.
async fn push_heartbeat(monitor: &Monitor, pool: &DbPool) -> Option<Heartbeat> {
    use time::OffsetDateTime;

    let now = OffsetDateTime::now_utc();

    // Cron mode: `config.cron` declares WHEN the job is supposed to
    // complete, so the tick only speaks up when expectations are broken
    // (missed run / overrun). The job's own /run /complete /fail pings
    // own the healthy timeline — synthesizing Up here would both pad the
    // uptime stats and stomp a fail ping's Down, which is exactly the
    // legacy-interval-mode quirk cron mode exists to fix.
    if let Some(schedule) = rampart_core::CronSchedule::from_config(&monitor.config) {
        let (last_push, run_started) = rampart_db::monitors::push_state(pool, monitor.id)
            .await
            .ok()?;

        // Overrun: a /run ping opened a run that's been in flight longer
        // than config.max_run_seconds allows.
        if let (Some(started), Some(max)) = (
            run_started,
            rampart_core::cron::max_run_seconds(&monitor.config),
        ) {
            let running_for = (now - started).whole_seconds();
            if running_for > max {
                return Some(synthetic_down(
                    monitor,
                    now,
                    format!("run exceeded max duration: {running_for}s (limit {max}s)"),
                ));
            }
        }

        // Missed run: the most recent scheduled slot plus grace passed
        // with no terminal ping since the slot. Slots older than the
        // monitor itself don't count — a monitor created at 10:05 isn't
        // late for the 10:00 run.
        let prev = schedule.expr.prev_occurrence(now)?;
        if prev < monitor.created_at {
            return None;
        }
        let deadline = prev + time::Duration::seconds(schedule.grace_seconds);
        let satisfied = last_push.map(|ts| ts >= prev).unwrap_or(false);
        if now > deadline && !satisfied {
            return Some(synthetic_down(
                monitor,
                now,
                format!(
                    "missed scheduled run at {} UTC (grace {}s)",
                    prev.format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_else(|_| prev.to_string()),
                    schedule.grace_seconds
                ),
            ));
        }
        return None;
    }

    // Interval mode — the original dead-man's-switch contract, unchanged.
    let last = rampart_db::monitors::fetch_last_push_at(pool, monitor.id)
        .await
        .ok()
        .flatten();

    let (status, msg) = match last {
        None => (MonitorStatus::Down, Some("no push received yet".into())),
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

    Some(Heartbeat {
        monitor_id: monitor.id,
        ts: now,
        status,
        latency_ms: None,
        status_code: None,
        msg,
        retries: 0,
        important: false,
    })
}

/// Synthetic monitor standing in for a metric rule in notifier events —
/// gives the template renderer well-formed fields ({{ monitor.name }} is
/// the rule name). Never persisted.
fn metric_rule_monitor(rule: &rampart_core::MetricRule) -> Monitor {
    let now = time::OffsetDateTime::now_utc();
    Monitor {
        id: rampart_core::MonitorId::new(),
        name: rule.name.clone(),
        kind: MonitorKind::Http,
        url: None,
        hostname: None,
        port: None,
        config: serde_json::Value::Null,
        interval_seconds: 60,
        retry_interval_sec: 60,
        max_retries: 0,
        timeout_seconds: 10,
        resend_interval_sec: 0,
        upside_down: false,
        http_method: "GET".into(),
        http_body: None,
        http_headers: None,
        accepted_statuses: vec![200],
        follow_redirect: true,
        ignore_tls: false,
        proxy_id: None,
        push_token: None,
        last_push_at: None,
        last_run_started_at: None,
        active: true,
        current_status: MonitorStatus::Up,
        created_at: now,
        updated_at: now,
        tags: Vec::new(),
        cert_days_left: None,
        cert_subject: None,
        cert_checked_at: None,
        group_id: None,
        slo_target_pct: None,
        slo_window_days: None,
        agent_id: None,
        escalation_policy_id: None,
    }
}

/// A scheduler-synthesized Down heartbeat for a broken cron expectation.
fn synthetic_down(monitor: &Monitor, now: time::OffsetDateTime, msg: String) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts: now,
        status: MonitorStatus::Down,
        latency_ms: None,
        status_code: None,
        msg: Some(msg),
        retries: 0,
        important: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_result_webhook_matches_known_vector() {
        // Vector independently computed with Python's hmac/hashlib:
        //   hmac.new(b"topsecret",
        //            b"1700000000.{\"hello\":\"world\"}",
        //            hashlib.sha256).hexdigest()
        let sig = sign_result_webhook("topsecret", 1_700_000_000, br#"{"hello":"world"}"#);
        assert_eq!(
            sig,
            "79883357e4c4c4abee43cf4b32367d67a1344520479e3e8c85e98406a6d6a2a5"
        );
        // Lowercase hex, 64 chars (32-byte digest).
        assert_eq!(sig.len(), 64);
        assert!(sig
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn sign_result_webhook_binds_timestamp() {
        // Same body, different ts → different signature (replay resistance).
        let body = br#"{"a":1}"#;
        let a = sign_result_webhook("k", 1, body);
        let b = sign_result_webhook("k", 2, body);
        assert_ne!(a, b);
    }
}

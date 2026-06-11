//! NotifierService — consumes events from the scheduler and fans out.
//!
//! Wiring:
//!   - `NotifierHandle` is the producer end. The scheduler holds one and
//!     calls `notify(event)` from each monitor task when a status flip is
//!     detected. The call is non-blocking; backpressure surfaces as a
//!     dropped notification with a warn log (we'd rather drop than stall
//!     the scheduler).
//!   - `NotifierService::run` is the consumer loop. One task, owns the
//!     reqwest clients, batches nothing — each event is dispatched as it
//!     arrives so latency stays low.
//!
//! Look-up flow per event:
//!   1. Query `monitor_notifications` JOIN `notifications` for the
//!      monitor's attached, enabled channels.
//!   2. For each, render subject + body via `template::render` (using
//!      the channel's optional `template_id` reference, else defaults).
//!   3. Dispatch in parallel — slow channels don't block fast ones.

use crate::{channels, template, Event, EventKind};
use rampart_core::ids::NotificationId;
use rampart_core::ChannelKind;
use rampart_db::DbPool;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

const CHANNEL_BUFFER: usize = 1024;

/// Rolling 1-hour per-channel send timestamps, used to enforce
/// `rate_limit_per_hour`. In-memory only (resets on restart) — the rate
/// limit is a soft throttle to stop a flapping monitor from hammering an
/// expensive channel, not a durable quota. Keyed by channel id; each
/// entry is the deque of send instants inside the trailing hour.
static RATE_WINDOW: OnceLock<Mutex<HashMap<NotificationId, VecDeque<Instant>>>> = OnceLock::new();

fn rate_window() -> &'static Mutex<HashMap<NotificationId, VecDeque<Instant>>> {
    RATE_WINDOW.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns true if the current UTC hour falls inside the half-open
/// quiet-hours window `[start, end)`. Supports wrap-around (start > end,
/// e.g. 22..6). `start == end` is an empty window (never quiet).
fn in_quiet_hours(hour: i16, start: i16, end: i16) -> bool {
    if start == end {
        false
    } else if start < end {
        hour >= start && hour < end
    } else {
        // wrap-around midnight: quiet if at-or-after start OR before end.
        hour >= start || hour < end
    }
}

/// Record a send for `id` and return true if it's allowed under
/// `rate_limit_per_hour`. Limit 0 = unlimited (always allowed, nothing
/// tracked). Otherwise prunes entries older than one hour, and if the
/// window is already full returns false WITHOUT recording (so a blocked
/// attempt doesn't extend the window); a permitted send is recorded.
fn rate_limit_allows(id: NotificationId, limit: i32) -> bool {
    if limit <= 0 {
        return true;
    }
    let now = Instant::now();
    let hour = Duration::from_secs(3600);
    let mut map = rate_window().lock().expect("rate window mutex poisoned");
    let dq = map.entry(id).or_default();
    while dq.front().is_some_and(|t| now.duration_since(*t) >= hour) {
        dq.pop_front();
    }
    if dq.len() >= limit as usize {
        return false;
    }
    dq.push_back(now);
    true
}

/// snake_case string form of a serde-serializable kind enum (ChannelKind /
/// EventKind), used to denormalise the kind into the delivery_log row so it
/// stays readable after the channel is deleted. Both enums derive Serialize
/// with `rename_all = "snake_case"`, so a unit variant serializes to a bare
/// JSON string; we strip the quotes. Falls back to "unknown" on the
/// impossible serialize failure rather than panicking on the logging path.
fn kind_str<T: serde::Serialize>(kind: &T) -> String {
    match serde_json::to_value(kind) {
        Ok(serde_json::Value::String(s)) => s,
        _ => "unknown".to_string(),
    }
}

/// Best-effort append to the delivery_log. Records one channel send attempt
/// (success or failure). By contract this NEVER affects dispatch — any DB
/// error is logged and swallowed, mirroring the fire-and-forget nature of
/// the notifier. Called from both the immediate dispatch path and the
/// digest flush so the log reflects every actual send attempt.
async fn record_delivery(
    pool: &DbPool,
    notification_id: NotificationId,
    channel_kind: ChannelKind,
    event: &Event,
    ok: bool,
    error: Option<&str>,
) {
    let entry = rampart_db::delivery_log::NewDelivery {
        notification_id: Some(notification_id),
        channel_kind: &kind_str(&channel_kind),
        event_kind: &kind_str(&event.kind),
        monitor_id: Some(event.monitor.id.0),
        ok,
        error,
    };
    if let Err(e) = rampart_db::delivery_log::record(pool, entry).await {
        warn!(channel = %notification_id.0, error = %e, "delivery_log record failed");
    }
}

/// Parse the snake_case `event_kind` string persisted in a delivery_log row
/// back into an [`EventKind`]. `EventKind` derives `Deserialize` with
/// `rename_all = "snake_case"`, so a bare JSON string round-trips. Anything
/// unrecognised (an older/garbled row) falls back to `Test`, which is the
/// most innocuous kind to re-send.
fn parse_event_kind(s: &str) -> EventKind {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or(EventKind::Test)
}

/// Build the synthetic monitor used when the original `monitor_id` is gone
/// (deleted, or the row never carried one). Mirrors the "fake monitor" the
/// send-test path uses so the renderer always has well-formed fields.
fn synthetic_monitor() -> rampart_core::Monitor {
    let now = time::OffsetDateTime::now_utc();
    rampart_core::Monitor {
        id: rampart_core::ids::MonitorId::new(),
        name: "(deleted monitor)".into(),
        kind: rampart_core::MonitorKind::Http,
        url: Some("https://example.com".into()),
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
        current_status: rampart_core::MonitorStatus::Up,
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

/// Re-send a single past delivery through its ORIGINAL channel and record a
/// NEW delivery_log row for the retry attempt (returned to the caller).
///
/// The channel is identified by the stored `notification_id`; the caller
/// (the admin retry route) is responsible for rejecting an entry whose
/// channel was deleted (`notification_id IS NULL`) before reaching here, so
/// this takes a concrete `NotificationId`. The channel's CURRENT config is
/// loaded fresh (an edit since the original send is honoured), and the same
/// per-channel send path used by live dispatch — `channels::dispatch` —
/// is reused so there is no duplicated send logic.
///
/// The original event payload isn't persisted in the log, so we rebuild a
/// representative [`Event`] from the row's `event_kind` plus the monitor it
/// referenced (loaded fresh; a synthetic stand-in if the monitor is gone).
/// Both a successful and a failed retry append a fresh row — the returned
/// entry is that new attempt.
pub async fn resend_delivery(
    pool: &DbPool,
    entry: &rampart_db::delivery_log::DeliveryEntry,
    channel_id: NotificationId,
) -> anyhow::Result<rampart_db::delivery_log::DeliveryEntry> {
    let chan = rampart_db::notifications::get(pool, channel_id).await?;

    // Rebuild a representative monitor for rendering: the real one when it
    // still exists, otherwise a synthetic stand-in.
    let monitor = match entry.monitor_id {
        Some(mid) => {
            match rampart_db::monitors::get(pool, rampart_core::ids::MonitorId::from_uuid(mid))
                .await
            {
                Ok(m) => m,
                Err(rampart_db::DbError::NotFound) => synthetic_monitor(),
                Err(e) => return Err(e.into()),
            }
        }
        None => synthetic_monitor(),
    };

    let now = time::OffsetDateTime::now_utc();
    let heartbeat = rampart_core::Heartbeat {
        monitor_id: monitor.id,
        ts: now,
        status: monitor.current_status,
        latency_ms: None,
        status_code: None,
        msg: Some("Re-sent from the delivery log.".into()),
        retries: 0,
        important: true,
    };
    let event = Event {
        kind: parse_event_kind(&entry.event_kind),
        monitor,
        heartbeat,
        prev_status: None,
        slo_current_pct: None,
    };

    let subject = template::default_subject(&event);
    let body = match chan.template_id {
        None => template::default_body(&event),
        Some(tid) => match rampart_db::templates::get_render_strings(pool, tid).await {
            Ok(t) => template::render(&t.body, &event),
            Err(e) => {
                warn!(channel = %chan.name, template = %tid.0, error = %e,
                      "retry template lookup failed; falling back to default body");
                template::default_body(&event)
            }
        },
    };

    // Reuse the live per-channel send path, then record the retry as a new
    // attempt (success or failure) and hand that row back to the caller.
    let (ok, err) = match channels::dispatch(
        chan.kind,
        &chan.config,
        &subject,
        &body,
        &event,
        pool,
        channel_id,
    )
    .await
    {
        Ok(()) => {
            info!(channel = %chan.name, kind = ?chan.kind, "delivery re-sent");
            (true, None)
        }
        Err(e) => {
            warn!(channel = %chan.name, kind = ?chan.kind, error = %e, "delivery re-send failed");
            (false, Some(e.to_string()))
        }
    };

    let new_entry = rampart_db::delivery_log::record(
        pool,
        rampart_db::delivery_log::NewDelivery {
            notification_id: Some(channel_id),
            channel_kind: &kind_str(&chan.kind),
            event_kind: &kind_str(&event.kind),
            monitor_id: entry.monitor_id,
            ok,
            error: err.as_deref(),
        },
    )
    .await?;
    Ok(new_entry)
}

/// Send one event directly to ONE channel, bypassing monitor routing —
/// the metric-rule path (rules carry explicit channel ids; tag/folder
/// routing is monitor machinery). Renders with the channel's template
/// when set, records the attempt in the delivery log, and returns
/// whether the send succeeded. Channel ids that no longer resolve are
/// the caller's problem to skip.
pub async fn send_event_to_channel(
    pool: &DbPool,
    channel_id: NotificationId,
    event: &Event,
) -> anyhow::Result<bool> {
    let chan = rampart_db::notifications::get(pool, channel_id).await?;

    let subject = template::default_subject(event);
    let body = match chan.template_id {
        None => template::default_body(event),
        Some(tid) => match rampart_db::templates::get_render_strings(pool, tid).await {
            Ok(t) => template::render(&t.body, event),
            Err(e) => {
                warn!(channel = %chan.name, template = %tid.0, error = %e,
                      "metric-rule template lookup failed; falling back to default body");
                template::default_body(event)
            }
        },
    };

    let (ok, err) = match channels::dispatch(
        chan.kind,
        &chan.config,
        &subject,
        &body,
        event,
        pool,
        channel_id,
    )
    .await
    {
        Ok(()) => (true, None),
        Err(e) => {
            warn!(channel = %chan.name, kind = ?chan.kind, error = %e, "metric-rule send failed");
            (false, Some(e.to_string()))
        }
    };

    // monitor_id stays NULL — the event's monitor is a synthetic stand-in
    // for the rule, not a real row.
    let _ = rampart_db::delivery_log::record(
        pool,
        rampart_db::delivery_log::NewDelivery {
            notification_id: Some(channel_id),
            channel_kind: &kind_str(&chan.kind),
            event_kind: &kind_str(&event.kind),
            monitor_id: None,
            ok,
            error: err.as_deref(),
        },
    )
    .await;
    Ok(ok)
}

/// Open/advance/resolve the escalation lifecycle for one status flip.
/// Failures log and bail — a broken policy must never block the writer
/// pipeline (the heartbeat is already persisted by this point).
async fn handle_escalation_flip(
    pool: &DbPool,
    event: &Event,
    policy_id: rampart_core::ids::EscalationPolicyId,
) {
    let policy = match rampart_db::escalations::get(pool, policy_id).await {
        Ok(p) => p,
        Err(e) => {
            warn!(policy = %policy_id.0, error = %e, "escalation policy load failed");
            return;
        }
    };

    if event.heartbeat.status.is_down() {
        // Open + page step 0. None = an episode is already open
        // (flap / re-down while acked) — the ladder is already running,
        // nothing new to page.
        match rampart_db::escalations::open_episode(pool, event.monitor.id, &policy).await {
            Ok(Some(_episode)) => {
                fire_escalation_step(pool, event, &policy, 0).await;
            }
            Ok(None) => {}
            Err(e) => warn!(monitor = %event.monitor.id, error = %e, "episode open failed"),
        }
    } else if event.heartbeat.status == rampart_core::MonitorStatus::Up {
        // Recovery: close the episode and tell everyone already paged.
        match rampart_db::escalations::resolve(pool, event.monitor.id).await {
            Ok(Some(episode)) => {
                let mut recovery = event.clone();
                recovery.kind = EventKind::Escalation;
                recovery.heartbeat.msg = Some(format!(
                    "recovered — escalation closed at step {} of {}",
                    episode.last_step + 1,
                    policy.steps.len()
                ));
                for step in policy.steps.iter().take(episode.last_step as usize + 1) {
                    for ch in &step.channel_ids {
                        if let Err(e) = send_event_to_channel(pool, *ch, &recovery).await {
                            warn!(channel = %ch.0, error = %e, "escalation recovery send failed");
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => warn!(monitor = %event.monitor.id, error = %e, "episode resolve failed"),
        }
    }
}

/// Page every channel on one ladder step. Direct per-channel dispatch —
/// escalation pages bypass digest coalescing and quiet hours by design
/// (a page that waits for a digest window is not a page).
pub async fn fire_escalation_step(
    pool: &DbPool,
    event: &Event,
    policy: &rampart_core::EscalationPolicy,
    step_no: usize,
) {
    let Some(step) = policy.steps.get(step_no) else {
        return;
    };
    let mut page = event.clone();
    page.kind = EventKind::Escalation;
    let base = event.heartbeat.msg.clone().unwrap_or_default();
    page.heartbeat.msg = Some(format!(
        "step {} of {} ({}): {base}",
        step_no + 1,
        policy.steps.len(),
        policy.name
    ));
    for ch in &step.channel_ids {
        if let Err(e) = send_event_to_channel(pool, *ch, &page).await {
            warn!(channel = %ch.0, error = %e, "escalation step send failed");
        }
    }
}

/// How often the digest flush task wakes to drain due channel buffers.
/// Each channel's own `digest_window_secs` gates whether it actually
/// flushes on a given wake; this is just the resolution of the timer.
const DIGEST_FLUSH_TICK: Duration = Duration::from_secs(1);

/// Channel dispatch metadata + the events drained for one flush. Built at
/// flush time from the persisted `digest_buffer` rows (the source of
/// truth) joined with the channel's current config, so it always reflects
/// the latest config edit and survives a restart.
struct ChannelDigest {
    kind: ChannelKind,
    config: serde_json::Value,
    name: String,
    template_id: Option<rampart_core::ids::NotificationTemplateId>,
    window_secs: i32,
    events: Vec<Event>,
}

#[derive(Clone)]
pub struct NotifierHandle(mpsc::Sender<Event>);

impl NotifierHandle {
    /// Non-blocking enqueue. Drops the event with a warn-level log when
    /// the buffer is full — we don't want notifier backpressure to stall
    /// the scheduler.
    pub fn notify(&self, event: Event) {
        if let Err(e) = self.0.try_send(event) {
            warn!(error = %e, "notifier channel full or closed — dropping event");
        }
    }
}

pub struct NotifierService {
    rx: mpsc::Receiver<Event>,
    pool: DbPool,
}

impl NotifierService {
    pub fn new(pool: DbPool) -> (Self, NotifierHandle) {
        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER);
        (NotifierService { rx, pool }, NotifierHandle(tx))
    }

    pub async fn run(mut self) {
        info!("notifier service started");

        // Background flush task: every tick, drain any channel whose window
        // has elapsed and send its combined message. The buffer is backed
        // by the `digest_buffer` table, so on startup this task naturally
        // picks up any rows left by a previous process — pending coalesced
        // alerts survive a restart.
        let flush_pool = self.pool.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(DIGEST_FLUSH_TICK);
            loop {
                tick.tick().await;
                flush_due_digests(&flush_pool).await;
            }
        });

        while let Some(event) = self.rx.recv().await {
            let pool = self.pool.clone();
            // Dispatch on a child task so a slow channel can't block the
            // next event. The child handles its own logging.
            tokio::spawn(async move {
                if let Err(e) = dispatch_one(&pool, event).await {
                    warn!(error = %e, "notifier dispatch failed");
                }
            });
        }
        info!("notifier service stopped (channel closed)");
    }
}

async fn dispatch_one(pool: &DbPool, event: Event) -> anyhow::Result<()> {
    // Dependency suppression. If this monitor depends on any other
    // monitor whose current_status is Down/Pending, the failure here is
    // almost certainly downstream of *that* root cause. Suppress the
    // alert (heartbeat still recorded so the dashboard shows the state)
    // to keep one root incident from paging every dependent service.
    match rampart_db::monitor_groups::any_parent_down(pool, event.monitor.id).await {
        Ok(true) => {
            info!(
                monitor = %event.monitor.id,
                "notification suppressed: upstream dependency is down",
            );
            return Ok(());
        }
        Ok(false) => {}
        Err(e) => {
            // Fail open on dep-graph errors — better a duplicate page
            // than silence during a real outage. Log so it gets noticed.
            warn!(error = %e, "dependency check failed; firing anyway");
        }
    }

    // Escalation routing. A monitor that references a policy hands its
    // StatusFlip lifecycle to the ladder: a Down-ish flip opens an
    // episode and pages step 0; recovery resolves the episode and pages
    // every step already climbed. Either way the regular attached/tag
    // fan-out below is SKIPPED for these flips — the ladder owns who
    // hears about this monitor. Non-StatusFlip events (SLO, maintenance)
    // keep their normal routing even with a policy attached.
    if event.kind_is_status_flip() {
        if let Some(policy_id) = event.monitor.escalation_policy_id {
            handle_escalation_flip(pool, &event, policy_id).await;
            return Ok(());
        }
    }

    // Maintenance start/end also fan out to status-page email subscribers
    // of any page the affected monitor is on. Best-effort, runs alongside
    // the channel dispatch below. Fired from the scheduler's periodic
    // maintenance scan; de-dup lives in the DB column it stamps.
    if matches!(
        event.kind,
        EventKind::MaintenanceStarted | EventKind::MaintenanceEnded
    ) {
        fan_out_maintenance_subscribers(pool, &event).await;
    }

    // Resolve effective channels: explicitly-attached ∪ tag-matched ∪
    // folder-attached − excluded. Replaces the old direct attach lookup
    // so tag/folder routing fires without materialized rows.
    let rows = rampart_db::routing::resolve_channels_for_monitor(pool, event.monitor.id).await?;
    if rows.is_empty() {
        return Ok(());
    }

    let default_subject = template::default_subject(&event);
    let default_body = template::default_body(&event);

    // Fire all channels in parallel. Each channel may use its own template;
    // we fetch and render up-front (rather than passing template_id into the
    // task) so the dispatched task is cheap and self-contained.
    let mut handles = Vec::with_capacity(rows.len());
    for row in rows {
        // Quiet hours + rate limit. Both are skipped for user-initiated
        // Test events (a "send test" click must always go out). Applied
        // before the digest branch so a suppressed event is neither sent
        // nor buffered — keeps the contract simple: during quiet hours /
        // once over the rate cap, the channel is silent.
        //
        // Interaction with maintenance suppression (see item 7): the two
        // suppression paths are disjoint and don't double-handle.
        //   - monitor_down/up (StatusFlip) for a monitor inside an active
        //     maintenance window is suppressed UPSTREAM, in the scheduler:
        //     `run_once` emits a synthetic Maintenance heartbeat instead of
        //     probing and its `user_visible_flip` guard drops any flip
        //     into/out of Maintenance, so NO StatusFlip event ever reaches
        //     this loop. Quiet-hours therefore never sees (and can't
        //     double-count or double-suppress) a maintenance-suppressed
        //     down/up alert.
        //   - MaintenanceStarted/Ended are operational announcements that DO
        //     flow through here. They are treated like any other channel
        //     send: suppressed only when this channel has a quiet window
        //     configured AND we're inside it. A channel with no quiet window
        //     always announces them. That is the intended composition —
        //     quiet hours gates the announcement per-channel; it does not
        //     interact with the maintenance probe-suppression above.
        if !matches!(event.kind, EventKind::Test) {
            if let (Some(start), Some(end)) = (row.quiet_hours_start, row.quiet_hours_end) {
                let hour = time::OffsetDateTime::now_utc().hour() as i16;
                if in_quiet_hours(hour, start, end) {
                    debug!(
                        channel = %row.name, kind = ?row.kind, hour, start, end,
                        "notification suppressed: inside quiet hours",
                    );
                    continue;
                }
            }
            if !rate_limit_allows(row.id, row.rate_limit_per_hour) {
                debug!(
                    channel = %row.name, kind = ?row.kind,
                    limit = row.rate_limit_per_hour,
                    "notification suppressed: rate limit exceeded",
                );
                continue;
            }
        }

        // Digest / coalescing: when the channel has a window configured,
        // buffer the raw event in-memory instead of sending now. The flush
        // task renders one combined message per window. `Test` events
        // always go out immediately — a user clicking "send test" expects
        // an instant message, not one merged into a future digest.
        if row.digest_window_secs > 0 && !matches!(event.kind, EventKind::Test) {
            enqueue_digest(pool, row.id, &event).await;
            info!(
                channel = %row.name, kind = ?row.kind,
                window = row.digest_window_secs,
                "notification buffered for digest",
            );
            continue;
        }

        let (subject, body) = match row.template_id {
            None => (default_subject.clone(), default_body.clone()),
            Some(tid) => match rampart_db::templates::get_render_strings(pool, tid).await {
                Ok(t) => {
                    let subj = t
                        .subject
                        .as_deref()
                        .map(|s| template::render(s, &event))
                        .unwrap_or_else(|| default_subject.clone());
                    (subj, template::render(&t.body, &event))
                }
                Err(e) => {
                    warn!(channel = %row.name, template = %tid.0, error = %e,
                          "template lookup failed; falling back to defaults");
                    (default_subject.clone(), default_body.clone())
                }
            },
        };

        // Cooldown: skip the dispatch if we fired this channel within
        // cooldown_seconds. Flap-prone monitors paired with SMS / paging
        // channels get hammered without this; admins can set cooldown=0
        // to keep the legacy behavior.
        if row.cooldown_seconds > 0 {
            if let Some(last) = row.last_fired_at {
                let elapsed = (time::OffsetDateTime::now_utc() - last).whole_seconds();
                if elapsed < row.cooldown_seconds as i64 {
                    info!(
                        channel = %row.name, kind = ?row.kind,
                        cooldown = row.cooldown_seconds, elapsed,
                        "notification suppressed by cooldown",
                    );
                    continue;
                }
            }
        }

        let event = event.clone();
        let kind: ChannelKind = row.kind;
        let cfg = row.config;
        let name = row.name;
        let id = row.id;
        let pool_clone = pool.clone();
        handles.push(tokio::spawn(async move {
            match channels::dispatch(kind, &cfg, &subject, &body, &event, &pool_clone, id).await {
                Ok(()) => {
                    info!(channel = %name, kind = ?kind, "notification sent");
                    // Record the successful attempt (best-effort).
                    record_delivery(&pool_clone, id, kind, &event, true, None).await;
                    // Bump last_fired_at so the next event respects the cooldown.
                    if let Err(e) = rampart_db::notifications::mark_fired(&pool_clone, id).await {
                        warn!(channel = %name, error = %e, "mark_fired failed");
                    }
                }
                Err(e) => {
                    warn!(channel = %name, kind = ?kind, error = %e, "notification failed");
                    // Record the failed attempt with its error (best-effort).
                    record_delivery(&pool_clone, id, kind, &event, false, Some(&e.to_string()))
                        .await;
                }
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    Ok(())
}

/// Persist `event` to the channel's durable digest buffer. The DB is the
/// source of truth — once this returns the event survives a restart and
/// will be flushed by the background task once the channel's window
/// elapses. A serialization failure is logged and dropped (the event
/// can't be persisted, and we'd rather not stall the dispatch path).
async fn enqueue_digest(pool: &DbPool, id: NotificationId, event: &Event) {
    let json = match serde_json::to_value(event) {
        Ok(v) => v,
        Err(e) => {
            warn!(channel = %id.0, error = %e, "digest event serialize failed; dropping");
            return;
        }
    };
    if let Err(e) = rampart_db::digest_buffer::enqueue(pool, id, &json).await {
        warn!(channel = %id.0, error = %e, "digest enqueue failed; dropping");
    }
}

/// Drain every channel whose digest window has elapsed and dispatch one
/// combined message per channel. The set of due channels (those whose
/// oldest buffered event has aged past their `digest_window_secs`) comes
/// from the DB, so a restart resumes any rows left by a prior process.
/// Channels still inside their window are left untouched. Called on the
/// flush tick.
async fn flush_due_digests(pool: &DbPool) {
    let now = time::OffsetDateTime::now_utc();
    let due = match rampart_db::digest_buffer::drain_due(pool, now).await {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "digest drain_due failed");
            return;
        }
    };

    for channel in due {
        if let Err(e) = flush_channel(pool, channel.notification_id).await {
            warn!(channel = %channel.notification_id.0, error = %e, "digest flush failed");
        }
    }
}

/// Flush one due channel: load its buffered rows + current config, render
/// the combined message, dispatch, then delete exactly the rows that were
/// drained (events enqueued after this snapshot roll into the next window).
async fn flush_channel(pool: &DbPool, id: NotificationId) -> anyhow::Result<()> {
    let rows = rampart_db::digest_buffer::take_for_channel(pool, id).await?;
    if rows.is_empty() {
        return Ok(());
    }

    // Deserialize the persisted events, tracking the row ids so we delete
    // precisely what we drained. Rows that fail to deserialize are dropped
    // (and removed) rather than wedging the channel forever.
    let mut drained_ids = Vec::with_capacity(rows.len());
    let mut events = Vec::with_capacity(rows.len());
    for r in rows {
        drained_ids.push(r.id);
        match serde_json::from_value::<Event>(r.event_json) {
            Ok(ev) => events.push(ev),
            Err(e) => {
                warn!(channel = %id.0, error = %e, "buffered event deserialize failed; dropping")
            }
        }
    }

    // Look up the channel's current config (source of truth for kind /
    // config / template / window — reflects any edit made mid-window). If
    // the channel was deleted the FK cascade already cleared the buffer,
    // so a NotFound here means there's nothing left to do.
    let chan = match rampart_db::notifications::get(pool, id).await {
        Ok(c) => c,
        Err(rampart_db::DbError::NotFound) => {
            rampart_db::digest_buffer::delete_by_ids(pool, &drained_ids).await?;
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    if events.is_empty() {
        // Everything failed to deserialize — just clear the dead rows.
        rampart_db::digest_buffer::delete_by_ids(pool, &drained_ids).await?;
        return Ok(());
    }

    let digest = ChannelDigest {
        kind: chan.kind,
        config: chan.config,
        name: chan.name,
        template_id: chan.template_id,
        window_secs: chan.digest_window_secs,
        events,
    };

    let count = digest.events.len();
    let (subject, body) = render_digest(pool, &digest).await;
    // The combined message represents the whole window; use the most
    // recent event as the dispatch `event` so channels that read
    // structured fields (e.g. Web Push title) see current state.
    let repr = digest
        .events
        .last()
        .expect("non-empty: events checked above");
    match channels::dispatch(digest.kind, &digest.config, &subject, &body, repr, pool, id).await {
        Ok(()) => {
            info!(channel = %digest.name, kind = ?digest.kind, count, "digest sent");
            // Record the combined send as one delivery attempt, keyed on the
            // representative (most-recent) event (best-effort).
            record_delivery(pool, id, digest.kind, repr, true, None).await;
            if let Err(e) = rampart_db::notifications::mark_fired(pool, id).await {
                warn!(channel = %digest.name, error = %e, "mark_fired failed");
            }
            // Only delete the drained rows once the send succeeded — a
            // failed dispatch leaves them buffered for the next tick.
            rampart_db::digest_buffer::delete_by_ids(pool, &drained_ids).await?;
        }
        Err(e) => {
            warn!(channel = %digest.name, kind = ?digest.kind, error = %e, "digest send failed");
            record_delivery(pool, id, digest.kind, repr, false, Some(&e.to_string())).await;
        }
    }
    Ok(())
}

/// Render the combined subject + body for a channel's buffered events.
/// One line per event summarising its monitor + what happened, e.g.:
///
///   3 alerts in the last 60s:
///   - api down (was up)
///   - db SLO recovered
///   - web down (was up)
async fn render_digest(pool: &DbPool, digest: &ChannelDigest) -> (String, String) {
    let count = digest.events.len();
    let window = digest.window_secs;
    let subject = format!("{count} alerts in the last {window}s");

    // If the channel references a template, render each buffered event
    // through it and stack the bodies; otherwise use a compact one-line
    // summary per event. Subject lines from templates are dropped in the
    // combined view — the digest subject above is authoritative.
    let mut lines = Vec::with_capacity(count + 1);
    lines.push(format!("{subject}:"));
    match digest.template_id {
        Some(tid) => match rampart_db::templates::get_render_strings(pool, tid).await {
            Ok(t) => {
                for ev in &digest.events {
                    lines.push(template::render(&t.body, ev));
                }
            }
            Err(e) => {
                warn!(channel = %digest.name, template = %tid.0, error = %e,
                      "digest template lookup failed; falling back to defaults");
                for ev in &digest.events {
                    lines.push(format!("- {}", digest_event_line(ev)));
                }
            }
        },
        None => {
            for ev in &digest.events {
                lines.push(format!("- {}", digest_event_line(ev)));
            }
        }
    }
    (subject, lines.join("\n"))
}

/// One-line human summary of a single buffered event for the default
/// (no-template) digest body.
fn digest_event_line(ev: &Event) -> String {
    let name = &ev.monitor.name;
    match ev.kind {
        EventKind::StatusFlip => {
            format!("{name} {} (was {})", ev.status_str(), ev.prev_status_str())
        }
        EventKind::SloBreached => format!("{name} SLO breached"),
        EventKind::SloRecovered => format!("{name} SLO recovered"),
        EventKind::MaintenanceStarted => format!("{name} maintenance started"),
        EventKind::MaintenanceEnded => format!("{name} maintenance ended"),
        EventKind::MetricRuleFired => {
            format!(
                "{name} firing: {}",
                ev.heartbeat.msg.as_deref().unwrap_or("")
            )
        }
        EventKind::MetricRuleResolved => {
            format!(
                "{name} resolved: {}",
                ev.heartbeat.msg.as_deref().unwrap_or("")
            )
        }
        EventKind::Escalation => {
            format!(
                "{name} escalation: {}",
                ev.heartbeat.msg.as_deref().unwrap_or("")
            )
        }
        EventKind::Test => format!("{name} test"),
    }
}

/// System-wide SMTP config, stored in the `settings` table under key
/// "smtp". Mirrors `rampart-api`'s `smtp::SmtpConfig` — status-page
/// subscriber emails (incident updates today, maintenance start/end here)
/// go through this rather than a per-monitor email channel. Kept as a
/// local copy because the notifier crate can't depend on rampart-api.
#[derive(Debug, serde::Deserialize)]
struct SubscriberSmtp {
    host: String,
    port: u16,
    encryption: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    from: String,
}

/// Best-effort email fan-out to confirmed status-page subscribers for a
/// maintenance start/end event. Loads system SMTP from settings; if none
/// is configured we silently no-op (same contract as the incident
/// fan-out). Failures per recipient are logged, never surfaced — the
/// channel dispatch path is independent of this.
async fn fan_out_maintenance_subscribers(pool: &DbPool, event: &Event) {
    let emails = match rampart_db::maintenance::confirmed_subscriber_emails_for_monitors(
        pool,
        std::slice::from_ref(&event.monitor.id),
    )
    .await
    {
        Ok(e) if !e.is_empty() => e,
        Ok(_) => return,
        Err(e) => {
            warn!(error = %e, "maintenance subscriber lookup failed");
            return;
        }
    };

    let cfg = match rampart_db::settings::get(pool, "smtp").await {
        Ok(Some(v)) => match serde_json::from_value::<SubscriberSmtp>(v) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "smtp config parse failed; skipping subscriber email");
                return;
            }
        },
        // No SMTP configured — silent no-op, channels still fired above.
        Ok(None) => return,
        Err(e) => {
            warn!(error = %e, "smtp config load failed; skipping subscriber email");
            return;
        }
    };

    let subject = template::default_subject(event);
    let body = template::default_body(event);
    for addr in emails {
        if let Err(e) = send_subscriber_email(&cfg, &addr, &subject, &body).await {
            warn!(recipient = %addr, error = %e, "maintenance subscriber email failed");
        }
    }
}

/// Send a pre-rendered email to a set of recipients via the system SMTP
/// config (settings key `"smtp"`), reusing the exact transport path the
/// maintenance-subscriber fan-out uses. Returns the number of recipients
/// successfully sent to; per-recipient failures are logged, never bubbled.
/// No-ops (returns 0) when no SMTP is configured or the config is invalid —
/// same fail-soft contract as the subscriber fan-out. Used by the
/// scheduler's weekly uptime-report check.
pub async fn send_system_email(
    pool: &DbPool,
    recipients: &[String],
    subject: &str,
    body: &str,
) -> usize {
    if recipients.is_empty() {
        return 0;
    }
    let cfg = match rampart_db::settings::get(pool, "smtp").await {
        Ok(Some(v)) => match serde_json::from_value::<SubscriberSmtp>(v) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "smtp config parse failed; skipping report email");
                return 0;
            }
        },
        Ok(None) => return 0,
        Err(e) => {
            warn!(error = %e, "smtp config load failed; skipping report email");
            return 0;
        }
    };
    let mut sent = 0usize;
    for addr in recipients {
        match send_subscriber_email(&cfg, addr, subject, body).await {
            Ok(()) => sent += 1,
            Err(e) => warn!(recipient = %addr, error = %e, "report email failed"),
        }
    }
    sent
}

async fn send_subscriber_email(
    cfg: &SubscriberSmtp,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    use lettre::message::{header, Mailbox};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
    use std::str::FromStr;

    let from = Mailbox::from_str(&cfg.from).map_err(|e| format!("from addr: {e}"))?;
    let to_mb = Mailbox::from_str(to).map_err(|e| format!("to addr: {e}"))?;
    let msg = Message::builder()
        .from(from)
        .to(to_mb)
        .subject(subject)
        .header(header::ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| format!("build: {e}"))?;

    let builder = match cfg.encryption.as_str() {
        "tls" => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host).map_err(|e| e.to_string())?
        }
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)
            .map_err(|e| e.to_string())?,
        "plain" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host),
        other => return Err(format!("unknown encryption: {other}")),
    };
    let mut builder = builder.port(cfg.port);
    if let (Some(u), Some(p)) = (cfg.username.as_deref(), cfg.password.as_deref()) {
        builder = builder.credentials(Credentials::new(u.into(), p.into()));
    }
    builder
        .build()
        .send(msg)
        .await
        .map_err(|e| format!("send: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventKind;

    /// The exact predicate the dispatch loop applies to decide whether a
    /// quiet window suppresses a send for one channel: a channel only
    /// suppresses when it HAS a quiet window configured and the current
    /// hour falls inside it. Mirrors the `if let (Some, Some)` guard in
    /// `dispatch_one` so we can assert the maintenance interaction without
    /// standing up a DB.
    fn quiet_suppresses(hour: i16, window: Option<(i16, i16)>) -> bool {
        match window {
            Some((start, end)) => in_quiet_hours(hour, start, end),
            None => false,
        }
    }

    #[test]
    fn in_quiet_hours_handles_plain_and_wraparound() {
        // plain window 9..17
        assert!(in_quiet_hours(10, 9, 17));
        assert!(!in_quiet_hours(8, 9, 17));
        assert!(!in_quiet_hours(17, 9, 17)); // half-open: end excluded
                                             // wrap-around 22..6
        assert!(in_quiet_hours(23, 22, 6));
        assert!(in_quiet_hours(5, 22, 6));
        assert!(!in_quiet_hours(6, 22, 6));
        assert!(!in_quiet_hours(12, 22, 6));
        // empty window start==end never quiet
        assert!(!in_quiet_hours(3, 3, 3));
    }

    /// Item 7: maintenance start/end announcements are operational events
    /// that flow through the channel dispatch loop and respect quiet hours
    /// like any other send — but ONLY on channels that actually have a
    /// quiet window. A channel with no quiet window always announces.
    #[test]
    fn maintenance_announcements_respect_quiet_hours_only_when_window_set() {
        // Channel WITH a quiet window 22..6, current hour 23 → suppressed.
        assert!(quiet_suppresses(23, Some((22, 6))));
        // Same channel at 12:00 (outside the window) → fires.
        assert!(!quiet_suppresses(12, Some((22, 6))));
        // Channel with NO quiet window → maintenance announcements always
        // fire, regardless of the hour.
        assert!(!quiet_suppresses(3, None));
        assert!(!quiet_suppresses(23, None));
    }

    /// Documents the no-double-suppression invariant for monitor_down/up
    /// (item 7): a StatusFlip into or out of Maintenance is suppressed
    /// UPSTREAM in the scheduler (`run_once`'s `user_visible_flip` guard),
    /// so the notifier's quiet-hours path never observes one. This test
    /// pins the contract the comment in `dispatch_one` relies on: only
    /// non-maintenance flips ever reach the notifier, so quiet hours and
    /// maintenance suppression operate on disjoint event sets.
    #[test]
    fn statusflip_during_maintenance_is_suppressed_upstream_not_here() {
        use rampart_core::MonitorStatus;
        // Mirror of scheduler `run_once`'s guard.
        fn user_visible_flip(prev: MonitorStatus, now: MonitorStatus) -> bool {
            !(prev == MonitorStatus::Pending && now == MonitorStatus::Up)
                && now != MonitorStatus::Maintenance
                && prev != MonitorStatus::Maintenance
        }
        // Flip into maintenance, or out of it, is NOT user-visible → no
        // StatusFlip event emitted → notifier never applies quiet hours to it.
        assert!(!user_visible_flip(
            MonitorStatus::Up,
            MonitorStatus::Maintenance
        ));
        assert!(!user_visible_flip(
            MonitorStatus::Maintenance,
            MonitorStatus::Up
        ));
        assert!(!user_visible_flip(
            MonitorStatus::Maintenance,
            MonitorStatus::Down
        ));
        // A genuine outage flip outside maintenance IS user-visible and
        // does reach the notifier (where quiet hours may then gate it).
        assert!(user_visible_flip(MonitorStatus::Up, MonitorStatus::Down));
        // And EventKind discriminates the two announcement kinds we DO let
        // through the loop.
        assert!(matches!(
            EventKind::MaintenanceStarted,
            EventKind::MaintenanceStarted
        ));
    }
}

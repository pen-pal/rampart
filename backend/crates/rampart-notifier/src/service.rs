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
                    // Bump last_fired_at so the next event respects the cooldown.
                    if let Err(e) = rampart_db::notifications::mark_fired(&pool_clone, id).await {
                        warn!(channel = %name, error = %e, "mark_fired failed");
                    }
                }
                Err(e) => warn!(channel = %name, kind = ?kind, error = %e, "notification failed"),
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
            if let Err(e) = rampart_db::notifications::mark_fired(pool, id).await {
                warn!(channel = %digest.name, error = %e, "mark_fired failed");
            }
            // Only delete the drained rows once the send succeeded — a
            // failed dispatch leaves them buffered for the next tick.
            rampart_db::digest_buffer::delete_by_ids(pool, &drained_ids).await?;
        }
        Err(e) => {
            warn!(channel = %digest.name, kind = ?digest.kind, error = %e, "digest send failed");
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

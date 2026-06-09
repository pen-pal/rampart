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
use rampart_core::ChannelKind;
use rampart_db::DbPool;
use tokio::sync::mpsc;
use tracing::{info, warn};

const CHANNEL_BUFFER: usize = 1024;

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

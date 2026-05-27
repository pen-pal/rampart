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

use crate::{channels, template, Event};
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
    let rows = rampart_db::notifications::for_monitor(pool, event.monitor.id).await?;
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
        let id   = row.id;
        let pool_clone = pool.clone();
        handles.push(tokio::spawn(async move {
            match channels::dispatch(kind, &cfg, &subject, &body, &event).await {
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

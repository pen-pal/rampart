//! Rampart remote probe agent.
//!
//! Pulls assigned monitors from a Rampart server, probes them locally on
//! their configured intervals with the same `rampart-checker` runners the
//! server uses, and reports heartbeats back in batches. The agent always
//! dials out — drop it behind NAT or a corporate firewall and it still
//! works with zero inbound connectivity.
//!
//! Env vars:
//!   RAMPART_URL              (required) base URL of the server, e.g. https://status.example.com
//!   RAMPART_AGENT_TOKEN      (required) `rmpa_…` token minted by POST /v1/agents
//!   RAMPART_AGENT_POLL_SECS  default 30 — how often to re-pull assignments
//!   RUST_LOG                 default "rampart_agent=info,info"
//!
//! Loop shape mirrors the server's scheduler in miniature: one tokio task
//! per monitor ticking on its interval, results funnelled into a channel,
//! and a reporter task that flushes batches (1s / 100 results, whichever
//! first) to POST /v1/agent/heartbeats. A pull failure keeps the current
//! task set running on the last-known config — a flaky link degrades to
//! stale assignments, never to dead probes.

use rampart_checker::Probes;
use rampart_core::agent::AgentResult;
use rampart_core::{Monitor, MonitorId, RetryBackoff};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const BATCH_SIZE: usize = 100;
const BATCH_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const RESULT_CHANNEL_BUFFER: usize = 1024;

struct Config {
    base_url: String,
    token: String,
    poll: Duration,
}

struct ProbeTask {
    cancel: Arc<Notify>,
    /// `updated_at` of the monitor snapshot the task was started with.
    /// A newer value on a later pull restarts the task with fresh config.
    updated_at: time::OffsetDateTime,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rampart_agent=info,info")),
        )
        .init();

    // Single decision point for the workspace's crypto stack — same as
    // rampart-api's main.rs. Must run before any reqwest client exists.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cfg = Config {
        base_url: std::env::var("RAMPART_URL")
            .map_err(|_| anyhow::anyhow!("RAMPART_URL not set"))?
            .trim_end_matches('/')
            .to_string(),
        token: std::env::var("RAMPART_AGENT_TOKEN")
            .map_err(|_| anyhow::anyhow!("RAMPART_AGENT_TOKEN not set"))?,
        poll: Duration::from_secs(
            std::env::var("RAMPART_AGENT_POLL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
        ),
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let client = Arc::new(client);
    let cfg = Arc::new(cfg);
    let probes = Arc::new(Probes::new());

    let (tx, rx) = mpsc::channel::<AgentResult>(RESULT_CHANNEL_BUFFER);

    // Reporter: batches results and POSTs them back.
    {
        let client = client.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move { reporter_loop(client, cfg, rx).await });
    }

    info!(server = %cfg.base_url, poll_s = cfg.poll.as_secs(), version = env!("CARGO_PKG_VERSION"), "rampart-agent starting");

    // Pull/reconcile loop — the main task.
    let mut tasks: HashMap<MonitorId, ProbeTask> = HashMap::new();
    loop {
        match pull_monitors(&client, &cfg).await {
            Ok(monitors) => reconcile(&mut tasks, monitors, &probes, &tx),
            Err(e) => warn!(error = %e, "assignment pull failed; keeping current task set"),
        }
        tokio::time::sleep(cfg.poll).await;
    }
}

async fn pull_monitors(client: &reqwest::Client, cfg: &Config) -> anyhow::Result<Vec<Monitor>> {
    let resp = client
        .get(format!("{}/v1/agent/monitors", cfg.base_url))
        .bearer_auth(&cfg.token)
        .header("x-rampart-agent-version", env!("CARGO_PKG_VERSION"))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!("server rejected the agent token (401) — was the agent revoked?");
    }
    Ok(resp.error_for_status()?.json::<Vec<Monitor>>().await?)
}

/// Diff the pulled assignment set against running tasks: stop tasks for
/// unassigned monitors, restart tasks whose config changed (detected via
/// `updated_at`), start tasks for new assignments.
fn reconcile(
    tasks: &mut HashMap<MonitorId, ProbeTask>,
    monitors: Vec<Monitor>,
    probes: &Arc<Probes>,
    tx: &mpsc::Sender<AgentResult>,
) {
    let fresh: HashMap<MonitorId, Monitor> = monitors.into_iter().map(|m| (m.id, m)).collect();

    // Stop: no longer assigned, or config changed (restart below).
    let to_stop: Vec<MonitorId> = tasks
        .iter()
        .filter(|(id, t)| {
            fresh
                .get(id)
                .map(|m| m.updated_at > t.updated_at)
                .unwrap_or(true)
        })
        .map(|(id, _)| *id)
        .collect();
    for id in to_stop {
        if let Some(t) = tasks.remove(&id) {
            t.cancel.notify_one();
            info!(monitor = %id, "probe task stopped");
        }
    }

    // Start anything assigned but not running.
    for (id, monitor) in fresh {
        if tasks.contains_key(&id) {
            continue;
        }
        let cancel = Arc::new(Notify::new());
        let updated_at = monitor.updated_at;
        let probes = probes.clone();
        let tx = tx.clone();
        let cancel_in_task = cancel.clone();
        info!(monitor = %id, name = %monitor.name, interval_s = monitor.interval_seconds, "probe task started");
        tokio::spawn(async move {
            probe_loop(monitor, probes, tx, cancel_in_task).await;
        });
        tasks.insert(id, ProbeTask { cancel, updated_at });
    }
}

/// One monitor's probe loop: immediate first fire, then tick on the
/// interval until cancelled. Config is fixed for the life of the task —
/// edits restart the task via the `updated_at` diff in `reconcile`.
async fn probe_loop(
    monitor: Monitor,
    probes: Arc<Probes>,
    tx: mpsc::Sender<AgentResult>,
    cancel: Arc<Notify>,
) {
    let interval = Duration::from_secs(monitor.interval_seconds.max(10) as u64);
    loop {
        let hb = probe_with_retries(&probes, &monitor).await;
        let result = AgentResult {
            monitor_id: monitor.id,
            status: hb.status,
            latency_ms: hb.latency_ms,
            status_code: hb.status_code,
            msg: hb.msg,
            retries: hb.retries,
        };
        if tx.send(result).await.is_err() {
            error!(monitor = %monitor.id, "result channel closed; probe task exiting");
            return;
        }
        tokio::select! {
            _ = cancel.notified() => return,
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

/// Same retry contract as the server's scheduler: a failed probe re-runs
/// up to `max_retries` times, sleeping the configured backoff curve
/// between attempts, before the failure is committed. Without a
/// `config.retry_backoff` block the first result stands as-is.
async fn probe_with_retries(probes: &Probes, monitor: &Monitor) -> rampart_core::Heartbeat {
    let mut hb = probes.run(monitor).await;
    if !hb.status.is_down() || monitor.max_retries <= 0 {
        return hb;
    }
    let Some(backoff) = RetryBackoff::from_config(&monitor.config) else {
        return hb;
    };
    for attempt in 1..=monitor.max_retries {
        tokio::time::sleep(Duration::from_secs(
            backoff.delay_secs(attempt as u32) as u64
        ))
        .await;
        hb = probes.run(monitor).await;
        hb.retries = attempt;
        if !hb.status.is_down() {
            break;
        }
    }
    hb
}

/// Collect results off the channel and flush batches to the server.
/// A failed POST re-queues nothing — the next probe tick regenerates
/// state, mirroring the server-side writer's "log loudly, keep going"
/// contract.
async fn reporter_loop(
    client: Arc<reqwest::Client>,
    cfg: Arc<Config>,
    mut rx: mpsc::Receiver<AgentResult>,
) {
    let mut buffer: Vec<AgentResult> = Vec::with_capacity(BATCH_SIZE);
    loop {
        let first = match rx.recv().await {
            Some(r) => r,
            None => return,
        };
        buffer.push(first);
        let flush_at = tokio::time::Instant::now() + BATCH_FLUSH_INTERVAL;
        while buffer.len() < BATCH_SIZE {
            tokio::select! {
                r = rx.recv() => match r {
                    Some(r) => buffer.push(r),
                    None => break,
                },
                _ = tokio::time::sleep_until(flush_at) => break,
            }
        }

        match client
            .post(format!("{}/v1/agent/heartbeats", cfg.base_url))
            .bearer_auth(&cfg.token)
            .json(&buffer)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) => error!(status = %resp.status(), batch = buffer.len(), "report rejected"),
            Err(e) => error!(error = %e, batch = buffer.len(), "report failed"),
        }
        buffer.clear();
    }
}

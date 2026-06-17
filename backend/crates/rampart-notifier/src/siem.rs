//! SIEM / syslog export.
//!
//! Streams the audit log — which is Rampart's security-event store (logins,
//! failed logins, 2FA failures, config changes) — to an external sink so a blue
//! team can ingest it into a real SIEM. A leader-gated forward tail with a
//! persisted cursor; best-effort: a down sink retries next tick and never
//! blocks the app. Two sinks: an HTTP **webhook** (POST a JSON array) and
//! **syslog** (UDP, one RFC5424 line per event). See `docs/design/SIEM.md`.

use rampart_db::leader::Leadership;
use rampart_db::DbPool;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

const CONFIG_KEY: &str = "siem_export";
const CURSOR_KEY: &str = "siem_export_cursor";
const FINDINGS_CURSOR_KEY: &str = "siem_export_findings_cursor";
const BATCH: i64 = 500;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiemConfig {
    #[serde(default)]
    pub enabled: bool,
    /// "webhook" (HTTP POST a JSON array) or "syslog" (UDP, RFC5424/event).
    #[serde(default)]
    pub kind: String,
    /// webhook: the URL. syslog: `host:port`.
    #[serde(default)]
    pub target: String,
}

pub async fn load_config(pool: &DbPool) -> SiemConfig {
    rampart_db::settings::get(pool, CONFIG_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

async fn cursor(pool: &DbPool) -> i64 {
    rampart_db::settings::get(pool, CURSOR_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

async fn set_cursor(pool: &DbPool, id: i64) {
    let _ = rampart_db::settings::put(pool, CURSOR_KEY, &serde_json::json!(id)).await;
}

/// Findings cursor is the `created_at` of the last forwarded finding, stored as
/// an RFC3339 string (the findings PK is a UUID, so we can't reuse the i64
/// cursor). `None` = forward from the beginning.
async fn findings_cursor(pool: &DbPool) -> Option<OffsetDateTime> {
    let raw = rampart_db::settings::get(pool, FINDINGS_CURSOR_KEY)
        .await
        .ok()
        .flatten()?;
    let s = raw.as_str()?;
    OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}

async fn set_findings_cursor(pool: &DbPool, ts: OffsetDateTime) {
    if let Ok(s) = ts.format(&time::format_description::well_known::Rfc3339) {
        let _ =
            rampart_db::settings::put(pool, FINDINGS_CURSOR_KEY, &serde_json::json!(s)).await;
    }
}

/// Spawnable loop. Polls every `interval`; when enabled + leader, forwards new
/// audit rows to the sink and advances the cursor only after a successful send.
pub async fn run_loop(pool: DbPool, leadership: Arc<Leadership>, interval: Duration) {
    let client = crate::http::client();
    loop {
        tokio::time::sleep(interval).await;
        if !leadership.is_leader() {
            continue;
        }
        let cfg = load_config(&pool).await;
        if !cfg.enabled || cfg.target.is_empty() {
            continue;
        }
        if let Err(e) = tick(&pool, &cfg, &client).await {
            tracing::warn!(error = %e, "siem export tick failed; will retry");
        }
    }
}

/// Drain every pending batch this tick (a backlog catches up in one pass).
/// Two independent forward tails share the one sink: the audit log (cursored by
/// the monotonic row id) and detection findings (cursored by `created_at`).
async fn tick(pool: &DbPool, cfg: &SiemConfig, client: &reqwest::Client) -> anyhow::Result<()> {
    // ── audit log ──────────────────────────────────────────────────
    loop {
        let after = cursor(pool).await;
        let rows = rampart_db::audit::fetch_since(pool, after, BATCH).await?;
        if rows.is_empty() {
            break;
        }
        let last = rows.last().map(|r| r.id).unwrap_or(after);
        send_rows(cfg, client, &rows, "audit").await?;
        set_cursor(pool, last).await;
        if (rows.len() as i64) < BATCH {
            break;
        }
    }

    // ── detection findings ─────────────────────────────────────────
    loop {
        let after = findings_cursor(pool).await;
        let rows = rampart_db::detection::fetch_since(pool, after, BATCH).await?;
        if rows.is_empty() {
            break;
        }
        let last = rows.last().map(|r| r.created_at);
        send_rows(cfg, client, &rows, "detection").await?;
        if let Some(ts) = last {
            set_findings_cursor(pool, ts).await;
        }
        if (rows.len() as i64) < BATCH {
            break;
        }
    }
    Ok(())
}

/// Dispatch a batch of any serializable security events to the configured sink.
async fn send_rows<T: Serialize>(
    cfg: &SiemConfig,
    client: &reqwest::Client,
    rows: &[T],
    app: &str,
) -> anyhow::Result<()> {
    match cfg.kind.as_str() {
        "syslog" => send_syslog(&cfg.target, rows, app).await,
        "syslog_tcp" => send_syslog_tcp(&cfg.target, rows, app).await,
        _ => send_webhook(client, &cfg.target, rows).await,
    }
}

async fn send_webhook<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    rows: &[T],
) -> anyhow::Result<()> {
    let resp = client.post(url).json(&rows).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("webhook {url} returned {}", resp.status());
    }
    Ok(())
}

async fn send_syslog<T: Serialize>(target: &str, rows: &[T], app: &str) -> anyhow::Result<()> {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(target).await?;
    for r in rows {
        sock.send(syslog_line(r, app)?.as_bytes()).await?;
    }
    Ok(())
}

/// Syslog over TCP (`host:port`), newline-framed RFC5424 — for collectors that
/// want a reliable stream rather than UDP datagrams (and the common path to a
/// TLS-terminating sidecar like stunnel). Native TLS is a follow-up.
async fn send_syslog_tcp<T: Serialize>(target: &str, rows: &[T], app: &str) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut stream = tokio::net::TcpStream::connect(target).await?;
    for r in rows {
        let mut line = syslog_line(r, app)?;
        line.push('\n');
        stream.write_all(line.as_bytes()).await?;
    }
    stream.flush().await?;
    Ok(())
}

/// One RFC5424 line for a security event. pri 134 = local0.info; nil "-"
/// timestamp is valid (the JSON payload carries the real `ts`). `app` is the
/// RFC5424 APP-NAME (`audit` or `detection`) so a collector can route by source.
fn syslog_line<T: Serialize>(r: &T, app: &str) -> anyhow::Result<String> {
    Ok(format!(
        "<134>1 - rampart {app} - - - {}",
        serde_json::to_string(r)?
    ))
}

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
    /// Wire format for each event: "json" (default, raw Rampart JSON), "cef"
    /// (ArcSight Common Event Format) or "leef" (QRadar Log Event Extended
    /// Format). Splunk/QRadar/ArcSight ingest CEF/LEEF natively; an absent or
    /// empty value is treated as "json" so existing configs are unchanged.
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "json".into()
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
/// Each row is rendered once into the configured wire format (JSON / CEF / LEEF)
/// and the resulting message strings are framed for the chosen transport.
async fn send_rows<T: Serialize>(
    cfg: &SiemConfig,
    client: &reqwest::Client,
    rows: &[T],
    app: &str,
) -> anyhow::Result<()> {
    let msgs: Vec<String> = rows
        .iter()
        .map(|r| {
            let v = serde_json::to_value(r).unwrap_or(serde_json::Value::Null);
            format_event(&v, app, &cfg.format)
        })
        .collect();
    match cfg.kind.as_str() {
        "syslog" => send_syslog(&cfg.target, &msgs, app).await,
        "syslog_tcp" => send_syslog_tcp(&cfg.target, &msgs, app).await,
        _ => send_webhook(client, &cfg.target, &msgs, &cfg.format).await,
    }
}

async fn send_webhook(
    client: &reqwest::Client,
    url: &str,
    msgs: &[String],
    format: &str,
) -> anyhow::Result<()> {
    // CEF/LEEF are line-oriented text; JSON keeps the historical array body.
    let req = if is_text_format(format) {
        client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(msgs.join("\n"))
    } else {
        client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(format!("[{}]", msgs.join(",")))
    };
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("webhook {url} returned {}", resp.status());
    }
    Ok(())
}

async fn send_syslog(target: &str, msgs: &[String], app: &str) -> anyhow::Result<()> {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(target).await?;
    for m in msgs {
        sock.send(syslog_frame(m, app).as_bytes()).await?;
    }
    Ok(())
}

/// Syslog over TCP (`host:port`), newline-framed RFC5424 — for collectors that
/// want a reliable stream rather than UDP datagrams (and the common path to a
/// TLS-terminating sidecar like stunnel). Native TLS is a follow-up.
async fn send_syslog_tcp(target: &str, msgs: &[String], app: &str) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut stream = tokio::net::TcpStream::connect(target).await?;
    for m in msgs {
        let mut line = syslog_frame(m, app);
        line.push('\n');
        stream.write_all(line.as_bytes()).await?;
    }
    stream.flush().await?;
    Ok(())
}

/// Frame one already-formatted message in RFC5424. pri 134 = local0.info; nil
/// "-" timestamp is valid (the payload carries the real time). `app` is the
/// APP-NAME (`audit` or `detection`) so a collector can route by source. The
/// MSG is the JSON object, CEF record, or LEEF record as configured.
fn syslog_frame(msg: &str, app: &str) -> String {
    format!("<134>1 - rampart {app} - - - {msg}")
}

const VENDOR: &str = "Rampart";
const PRODUCT: &str = "Rampart";

fn is_text_format(format: &str) -> bool {
    matches!(format, "cef" | "leef")
}

/// Render one JSON-ified event into the configured wire format. Anything other
/// than "cef"/"leef" (including "json" and an empty legacy value) → raw JSON,
/// preserving the original behaviour.
fn format_event(v: &serde_json::Value, app: &str, format: &str) -> String {
    match format {
        "cef" => cef_line(v, app),
        "leef" => leef_line(v, app),
        _ => v.to_string(),
    }
}

/// Friendly-aliased, null-stripped (key, value) pairs shared by CEF + LEEF.
/// `ip_addr` is aliased to the standard `src` key; everything else keeps its
/// Rampart field name (SIEMs accept custom extension keys). Non-scalar values
/// (payload/sample) are emitted as compact JSON.
fn extension_pairs(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<(String, String)> {
    obj.iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            let key = match k.as_str() {
                "ip_addr" => "src",
                other => other,
            }
            .to_string();
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (key, val)
        })
        .collect()
}

/// CEF severity 0-10 from a `severity` field (string buckets or a number).
fn cef_severity(v: &serde_json::Value) -> u8 {
    if let Some(n) = v.as_i64() {
        return n.clamp(0, 10) as u8;
    }
    match v.as_str().unwrap_or("").to_ascii_lowercase().as_str() {
        "critical" => 10,
        "high" => 8,
        "medium" => 6,
        "low" => 3,
        "info" | "informational" => 2,
        _ => 5,
    }
}

fn cef_header_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|")
}

fn cef_value_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('=', "\\=")
}

/// `CEF:0|Vendor|Product|Version|SignatureID|Name|Severity|Extension`.
fn cef_line(v: &serde_json::Value, app: &str) -> String {
    let obj = v.as_object();
    let pick = |keys: &[&str]| -> Option<String> {
        let o = obj?;
        keys.iter()
            .find_map(|k| o.get(*k).and_then(|x| x.as_str()))
            .map(str::to_string)
    };
    let name = pick(&["action", "rule_name", "rule_id"]).unwrap_or_else(|| app.to_string());
    let sig = pick(&["rule_id", "action"]).unwrap_or_else(|| app.to_string());
    let sev = obj
        .and_then(|o| o.get("severity"))
        .map(cef_severity)
        .unwrap_or(if app == "detection" { 6 } else { 3 });
    let ext = obj
        .map(|o| {
            extension_pairs(o)
                .iter()
                .map(|(k, val)| format!("{k}={}", cef_value_escape(val)))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    format!(
        "CEF:0|{VENDOR}|{PRODUCT}|{}|{}|{}|{sev}|{ext}",
        env!("CARGO_PKG_VERSION"),
        cef_header_escape(&sig),
        cef_header_escape(&name),
    )
}

fn leef_value_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// `LEEF:1.0|Vendor|Product|Version|EventID|<tab-delimited key=value attrs>`.
fn leef_line(v: &serde_json::Value, app: &str) -> String {
    let obj = v.as_object();
    let event_id = obj
        .and_then(|o| {
            o.get("action")
                .or_else(|| o.get("rule_id"))
                .and_then(|x| x.as_str())
        })
        .unwrap_or(app);
    let attrs = obj
        .map(|o| {
            extension_pairs(o)
                .iter()
                .map(|(k, val)| format!("{k}={}", leef_value_escape(val)))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .unwrap_or_default();
    format!(
        "LEEF:1.0|{VENDOR}|{PRODUCT}|{}|{}|{attrs}",
        env!("CARGO_PKG_VERSION"),
        cef_header_escape(event_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cef_audit_event() {
        let v = json!({
            "id": 7,
            "action": "login.failed",
            "resource_kind": "session",
            "ip_addr": "203.0.113.9",
            "actor_user_id": null,
        });
        let line = cef_line(&v, "audit");
        // header: CEF:0|Rampart|Rampart|<ver>|<sig=action>|<name=action>|<sev>
        assert!(line.starts_with(&format!(
            "CEF:0|Rampart|Rampart|{}|login.failed|login.failed|3|",
            env!("CARGO_PKG_VERSION")
        )));
        // ip_addr is aliased to the standard `src` extension key; nulls dropped.
        assert!(line.contains("src=203.0.113.9"));
        assert!(!line.contains("actor_user_id"));
        assert!(line.contains("resource_kind=session"));
    }

    #[test]
    fn cef_finding_severity_maps() {
        let v = json!({"rule_id": "r-1", "rule_name": "Brute force", "severity": "high"});
        let line = cef_line(&v, "detection");
        assert!(line.starts_with(&format!(
            "CEF:0|Rampart|Rampart|{}|r-1|Brute force|8|",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[test]
    fn cef_escapes_header_and_extension() {
        let v = json!({"action": "a|b", "note": "x=y\nz"});
        let line = cef_line(&v, "audit");
        assert!(line.contains("a\\|b")); // pipe escaped in header
        assert!(line.contains("note=x\\=y\\nz")); // = and newline escaped in extension
    }

    #[test]
    fn leef_event_tab_delimited() {
        let v = json!({"action": "config.update", "resource_kind": "monitor"});
        let line = leef_line(&v, "audit");
        assert!(line.starts_with(&format!(
            "LEEF:1.0|Rampart|Rampart|{}|config.update|",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(line.contains("action=config.update"));
        assert!(line.contains('\t')); // attributes are tab-delimited
    }

    #[test]
    fn format_event_defaults_to_json() {
        let v = json!({"action": "x"});
        // empty (legacy) and "json" both yield raw JSON, not CEF/LEEF.
        assert_eq!(format_event(&v, "audit", ""), v.to_string());
        assert_eq!(format_event(&v, "audit", "json"), v.to_string());
        assert!(format_event(&v, "audit", "cef").starts_with("CEF:0|"));
        assert!(format_event(&v, "audit", "leef").starts_with("LEEF:1.0|"));
    }

    #[test]
    fn cef_severity_buckets_and_numbers() {
        assert_eq!(cef_severity(&json!("critical")), 10);
        assert_eq!(cef_severity(&json!("low")), 3);
        assert_eq!(cef_severity(&json!("nonsense")), 5);
        assert_eq!(cef_severity(&json!(99)), 10); // clamped
        assert_eq!(cef_severity(&json!(4)), 4);
    }
}

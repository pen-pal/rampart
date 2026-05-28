//! Browser-rendered monitor.
//!
//! Sits between the HTTP probe and a real browser engine: we forward the
//! check to an external rendering service so the rampart image stays
//! lean (no Chromium binary, no 150MB+ runtime cost). The expected
//! service is browserless/chrome (or compatible) running on the same
//! network — Rampart sends a POST and asserts on the returned HTML.
//!
//! `monitor.url` — page to render.
//! `monitor.config`:
//!   * `renderer_url`  — full POST endpoint (e.g.
//!                       `http://browserless:3000/content`)
//!   * `keyword`       — required substring in rendered HTML
//!   * `keyword_invert`— bool, optional. When true, success requires the
//!                       keyword to be ABSENT (useful for "page must not
//!                       contain error banner")
//!   * `token`         — optional bearer token forwarded to the renderer
//!                       (browserless's API auth)
//!   * `wait_selector` — optional CSS selector to wait for before
//!                       capturing HTML (forwarded as `waitForSelector`)
//!
//! Timeout: monitor.timeout_seconds. We add a small buffer to the
//! renderer's own internal timeout so we win the race when the page
//! genuinely hangs.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use serde_json::json;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

pub struct BrowserProbe {
    client: reqwest::Client,
}

impl BrowserProbe {
    pub fn new() -> Self {
        // Same default-pool client as the http probe; reused for every
        // browser monitor.
        Self {
            client: reqwest::Client::builder()
                .user_agent("Rampart/0.4")
                .build()
                .expect("reqwest client"),
        }
    }
}

impl Default for BrowserProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for BrowserProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let start = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let url = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u,
            _ => return fail(monitor, ts, "monitor.url is required for browser kind"),
        };
        let cfg = &monitor.config;
        let renderer = cfg.get("renderer_url").and_then(|v| v.as_str());
        let keyword  = cfg.get("keyword").and_then(|v| v.as_str());
        let invert   = cfg
            .get("keyword_invert")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let token         = cfg.get("token").and_then(|v| v.as_str());
        let wait_selector = cfg.get("wait_selector").and_then(|v| v.as_str());

        let renderer = match renderer {
            Some(r) if !r.is_empty() => r,
            _ => return fail(monitor, ts, "config.renderer_url is required"),
        };
        let keyword = match keyword {
            Some(k) if !k.is_empty() => k,
            _ => return fail(monitor, ts, "config.keyword is required"),
        };

        // browserless /content payload. Optional gotoOptions controls
        // navigation timeout; we add a slack second so the renderer's
        // own deadline fires before ours.
        let timeout_ms = (monitor.timeout_seconds as u64).saturating_mul(1000);
        let mut body = json!({
            "url": url,
            "gotoOptions": {
                "waitUntil": "networkidle2",
                "timeout":   timeout_ms,
            },
        });
        if let Some(s) = wait_selector {
            body["waitForSelector"] = json!({ "selector": s, "timeout": timeout_ms });
        }

        let mut req = self.client.post(renderer).json(&body);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let req = req.timeout(Duration::from_millis(timeout_ms.saturating_add(1500)));

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                return fail(
                    monitor,
                    ts,
                    &format!("renderer call failed: {e}"),
                );
            }
        };
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let latency = ms_i32(start.elapsed());

        if !status.is_success() {
            return Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Down,
                latency_ms: Some(latency),
                status_code: Some(status.as_u16() as i32),
                msg: Some(format!(
                    "renderer returned {}: {}",
                    status.as_u16(),
                    truncate(&text, 200)
                )),
                retries: 0,
                important: false,
            };
        }

        let contains = text.contains(keyword);
        // upside_down: success/failure inverted by the monitor. Applied
        // *after* the keyword check so the keyword semantics stay
        // intuitive ("keyword absent" is its own flag).
        let raw_ok = if invert { !contains } else { contains };
        let ok = if monitor.upside_down { !raw_ok } else { raw_ok };

        let (s, msg) = if ok {
            (MonitorStatus::Up, None)
        } else if invert {
            (
                MonitorStatus::Down,
                Some(format!("keyword '{keyword}' unexpectedly present in rendered HTML")),
            )
        } else {
            (
                MonitorStatus::Down,
                Some(format!("keyword '{keyword}' not found in rendered HTML")),
            )
        };
        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: s,
            latency_ms: Some(latency),
            status_code: Some(status.as_u16() as i32),
            msg,
            retries: 0,
            important: false,
        }
    }
}

fn fail(monitor: &Monitor, ts: OffsetDateTime, msg: &str) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status: MonitorStatus::Down,
        latency_ms: None,
        status_code: None,
        msg: Some(msg.into()),
        retries: 0,
        important: false,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

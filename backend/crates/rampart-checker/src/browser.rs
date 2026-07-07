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
//! * `renderer_url` — full POST endpoint (e.g. `http://browserless:3000/content`)
//! * `keyword` — required substring in rendered HTML
//! * `keyword_invert` — bool, optional. When true, success requires the
//!   keyword to be ABSENT (useful for "page must not contain error banner")
//! * `token` — optional bearer token forwarded to the renderer (browserless API auth)
//! * `wait_selector` — optional CSS selector to wait for before capturing
//!   HTML (forwarded as `waitForSelector`)
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
    /// Operator-trusted renderer hosts (env `RAMPART_BROWSER_RENDERER_ALLOW`,
    /// comma-separated) that may resolve to PRIVATE IPs even under
    /// `RAMPART_SSRF_BLOCK_PRIVATE`. The always-blocked set (loopback /
    /// link-local / cloud metadata) is NEVER exemptable.
    allow_hosts: Vec<String>,
}

impl BrowserProbe {
    pub fn new() -> Self {
        // `renderer_url` is per-monitor config an EDITOR sets — NOT operator-only
        // — so the renderer connection IS SSRF-guarded: an editor could otherwise
        // point it at `http://169.254.169.254/…` or an internal port and read the
        // response body back. The client's resolver vets every dialled IP (closing
        // the DNS-rebind window), and a per-run preflight rejects with a clear
        // reason. An operator running a private internal renderer (e.g.
        // `browserless` on a 10.x host) allow-lists it via
        // `RAMPART_BROWSER_RENDERER_ALLOW`; even then metadata/loopback stay blocked.
        let allow_hosts: Vec<String> = std::env::var("RAMPART_BROWSER_RENDERER_ALLOW")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        Self {
            client: crate::ssrf::guarded_client_builder_allowing(allow_hosts.clone())
                .user_agent("Rampart/0.4")
                .build()
                .expect("reqwest client"),
            allow_hosts,
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
        let keyword = cfg.get("keyword").and_then(|v| v.as_str());
        let invert = cfg
            .get("keyword_invert")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let token = cfg.get("token").and_then(|v| v.as_str());
        let wait_selector = cfg.get("wait_selector").and_then(|v| v.as_str());

        let renderer = match renderer {
            Some(r) if !r.is_empty() => r,
            _ => return fail(monitor, ts, "config.renderer_url is required"),
        };

        // SSRF preflight on the RENDERER endpoint itself. `renderer_url` is
        // editor-settable monitor config, so an editor could aim it at an
        // internal port or `http://169.254.169.254/…` and exfil the response.
        // Operator-allow-listed hosts may be private; metadata/loopback are
        // always blocked. The client's resolver also vets the dialled IP, so a
        // DNS rebind that passes this preflight still can't connect to a blocked
        // address.
        if let Err(b) = crate::ssrf::guard_url_allowing(renderer, &self.allow_hosts).await {
            return fail(
                monitor,
                ts,
                &format!("SSRF blocked renderer {}: {}", b.host, b.reason),
            );
        }

        // SSRF preflight on the TARGET page. The renderer fetches monitor.url on
        // our behalf, so a url pointing at a link-local / metadata / private
        // address (e.g. http://169.254.169.254/…) is the same SSRF the HTTP probe
        // already blocks. Honors RAMPART_SSRF_BLOCK_PRIVATE (no-op when unset).
        // (The renderer endpoint itself is guarded just above.)
        if let Err(b) = crate::ssrf::guard_url(url).await {
            return fail(
                monitor,
                ts,
                &format!("SSRF blocked target {}: {}", b.host, b.reason),
            );
        }
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
                return fail(monitor, ts, &format!("renderer call failed: {e}"));
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
        // `invert` is the per-monitor `keyword_invert` flag (success = keyword
        // ABSENT) — distinct from monitor-level `upside_down`, which is applied
        // centrally in `Probes::run` so every kind honours it.
        let ok = if invert { !contains } else { contains };

        let (s, msg) = if ok {
            (MonitorStatus::Up, None)
        } else if invert {
            (
                MonitorStatus::Down,
                Some(format!(
                    "keyword '{keyword}' unexpectedly present in rendered HTML"
                )),
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
        // `n` is a byte budget, but `s` is remote response text (arbitrary
        // UTF-8) — slicing `&s[..n]` panics if n lands mid-multibyte-char, and
        // under `panic = "abort"` that aborts the whole server. Walk back to the
        // nearest char boundary first.
        let mut end = n;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

#[cfg(test)]
mod truncate_tests {
    #[test]
    fn truncate_multibyte_no_panic() {
        // n lands inside a 3-byte '…' — must not panic, must stay valid UTF-8.
        let s = "abc…def"; // '…' is 3 bytes at indices 3..6
        for n in 0..s.len() {
            let out = super::truncate(s, n);
            assert!(out.is_char_boundary(out.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::ids::MonitorId;
    use rampart_core::MonitorKind;

    fn browser_monitor(renderer: &str) -> Monitor {
        let now = OffsetDateTime::now_utc();
        Monitor {
            id: MonitorId::new(),
            name: "b".into(),
            kind: MonitorKind::Browser,
            url: Some("https://example.com".into()),
            hostname: None,
            port: None,
            config: json!({ "renderer_url": renderer, "keyword": "ok" }),
            interval_seconds: 60,
            retry_interval_sec: 60,
            max_retries: 0,
            timeout_seconds: 5,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: None,
            accepted_statuses: vec![],
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
            check_cert: false,
            cert_expiry_days: 14,
            group_id: None,
            slo_target_pct: None,
            slo_window_days: None,
            agent_id: None,
            escalation_policy_id: None,
        }
    }

    /// SECURITY (audit #123): the browser probe's renderer connection is now
    /// SSRF-guarded. An editor-set `renderer_url` aimed at cloud metadata is
    /// rejected BEFORE any request — `169.254.169.254` is always-blocked
    /// regardless of `RAMPART_SSRF_BLOCK_PRIVATE` and can never be allow-listed —
    /// instead of POSTing to it and reading the response body back.
    #[tokio::test]
    async fn renderer_url_to_cloud_metadata_is_ssrf_blocked() {
        // The real app installs the ring CryptoProvider at boot (main.rs); the
        // unit-test process must do the same or `reqwest::build()` panics
        // "No provider set" before any SSRF logic runs.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let probe = BrowserProbe::new();
        let hb = probe
            .run(&browser_monitor("http://169.254.169.254/latest/meta-data/"))
            .await;
        assert_eq!(hb.status, MonitorStatus::Down);
        assert!(
            hb.msg
                .as_deref()
                .unwrap_or("")
                .contains("SSRF blocked renderer"),
            "expected renderer SSRF block, got: {:?}",
            hb.msg
        );
    }
}

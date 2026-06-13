//! Synthetic transaction probe.
//!
//! Runs a `synthetic` monitor's `config.steps` sequence: for each step it
//! interpolates `{{var}}` placeholders from the accumulated variable bag,
//! sends the request, extracts values into the bag, and evaluates the step's
//! assertions. The first failed assertion (or transport error) stops the run
//! and produces a Down heartbeat naming the failing step; a clean sweep is Up
//! with the total wall-clock as latency.
//!
//! v1 limitations: no automatic cookie jar (carry session state explicitly
//! via extract → `{{var}}` header), and `upside_down` is not applied (it has
//! no clean meaning for a multi-assertion sequence).

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::synthetic::{
    compare, interpolate, json_lookup, AssertKind, Assertion, CompareOp, ExtractSource,
    SyntheticPlan, SyntheticStep,
};
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use reqwest::{Client, ClientBuilder, Method};
use serde_json::Value;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

const USER_AGENT: &str = concat!(
    "Rampart/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/pen-pal/rampart)"
);

/// Cap on bytes read from each step's response body (matches the HTTP probe).
const MAX_BODY_BYTES: usize = 524_288;

pub struct SyntheticProbe;

impl SyntheticProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SyntheticProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for SyntheticProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();

        let plan = match SyntheticPlan::from_config(&monitor.config) {
            Some(p) => p,
            None => {
                return down(
                    monitor,
                    ts,
                    started,
                    None,
                    "synthetic config invalid or empty (expected a non-empty config.steps)",
                )
            }
        };

        let client = match build_client(monitor) {
            Ok(c) => c,
            Err(e) => {
                return down(
                    monitor,
                    ts,
                    started,
                    None,
                    &format!("client build failed: {e}"),
                )
            }
        };

        let mut vars: BTreeMap<String, String> = BTreeMap::new();
        let mut last_status: Option<i32> = None;

        for (i, step) in plan.steps.iter().enumerate() {
            let label = step_label(i, step);

            let method = Method::from_str(
                interpolate(&step.method, &vars)
                    .trim()
                    .to_uppercase()
                    .as_str(),
            )
            .unwrap_or(Method::GET);
            let url = interpolate(&step.url, &vars);
            // SSRF guard each step's (interpolated) URL before issuing it.
            if let Ok(parsed) = reqwest::Url::parse(&url) {
                if let Some(host) = parsed.host_str() {
                    let port = parsed.port_or_known_default().unwrap_or(80);
                    if let Err(blocked) = crate::ssrf::resolve_guarded(
                        host,
                        port,
                        crate::ssrf::block_private_enabled(),
                    )
                    .await
                    {
                        return down(monitor, ts, started, None, &blocked.to_string());
                    }
                }
            }
            let mut req = client.request(method, &url);
            for (k, v) in &step.headers {
                req = req.header(k, interpolate(v, &vars));
            }
            if let Some(body) = &step.body {
                req = req.body(interpolate(body, &vars));
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) if e.is_timeout() => {
                    return down(
                        monitor,
                        ts,
                        started,
                        last_status,
                        &format!("{label}: request timed out"),
                    )
                }
                Err(e) => {
                    return down(
                        monitor,
                        ts,
                        started,
                        last_status,
                        &format!("{label}: request failed: {e}"),
                    )
                }
            };

            let status_code = resp.status().as_u16() as i32;
            last_status = Some(status_code);
            // Header names lowercased for case-insensitive lookup.
            let headers: BTreeMap<String, String> = resp
                .headers()
                .iter()
                .filter_map(|(k, v)| {
                    v.to_str()
                        .ok()
                        .map(|s| (k.as_str().to_ascii_lowercase(), s.to_string()))
                })
                .collect();
            let body: String = resp
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(MAX_BODY_BYTES)
                .collect();
            let json_body: Option<Value> = serde_json::from_str(&body).ok();

            // Extractions feed later steps. A miss leaves the var unset (the
            // placeholder stays literal downstream), surfacing the problem.
            for e in &step.extract {
                let value = match e.from {
                    ExtractSource::Status => Some(status_code.to_string()),
                    ExtractSource::Header => e
                        .path
                        .as_deref()
                        .and_then(|p| headers.get(&p.to_ascii_lowercase()).cloned()),
                    ExtractSource::Json => match (&json_body, e.path.as_deref()) {
                        (Some(j), Some(p)) => json_lookup(j, p),
                        _ => None,
                    },
                };
                if let Some(v) = value {
                    vars.insert(e.var.clone(), v);
                }
            }

            // First failed assertion stops the run.
            for a in &step.assert {
                if let Some(reason) = assertion_failure(a, status_code, &headers, &json_body, &body)
                {
                    return down(
                        monitor,
                        ts,
                        started,
                        Some(status_code),
                        &format!("{label}: {reason}"),
                    );
                }
            }
        }

        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: MonitorStatus::Up,
            latency_ms: Some(ms_i32(started.elapsed())),
            status_code: last_status,
            msg: None,
            retries: 0,
            important: false,
        }
    }
}

fn build_client(monitor: &Monitor) -> reqwest::Result<Client> {
    let redirect = if monitor.follow_redirect {
        reqwest::redirect::Policy::limited(10)
    } else {
        reqwest::redirect::Policy::none()
    };
    ClientBuilder::new()
        .user_agent(USER_AGENT)
        .redirect(redirect)
        .danger_accept_invalid_certs(monitor.ignore_tls)
        .timeout(Duration::from_secs(monitor.timeout_seconds as u64))
        .build()
}

fn step_label(i: usize, step: &SyntheticStep) -> String {
    match &step.name {
        Some(n) if !n.trim().is_empty() => format!("step {} ({})", i + 1, n.trim()),
        _ => format!("step {}", i + 1),
    }
}

/// Returns `Some(reason)` when the assertion FAILS, `None` when it passes.
fn assertion_failure(
    a: &Assertion,
    status: i32,
    headers: &BTreeMap<String, String>,
    json_body: &Option<Value>,
    body: &str,
) -> Option<String> {
    // body_contains is a raw substring check with no target lookup.
    if a.kind == AssertKind::BodyContains {
        return if body.contains(&a.value) {
            None
        } else {
            Some(format!("body does not contain {:?}", a.value))
        };
    }

    let (actual, target): (Option<String>, String) = match a.kind {
        AssertKind::Status => (Some(status.to_string()), "status".to_string()),
        AssertKind::Header => {
            let name = a.path.clone().unwrap_or_default();
            (
                headers.get(&name.to_ascii_lowercase()).cloned(),
                format!("header {name}"),
            )
        }
        AssertKind::Json => {
            let path = a.path.clone().unwrap_or_default();
            (
                json_body.as_ref().and_then(|j| json_lookup(j, &path)),
                format!("json {path}"),
            )
        }
        AssertKind::BodyContains => unreachable!("handled above"),
    };

    match actual {
        None => Some(format!("{target} not found")),
        Some(got) if compare(&got, a.op, &a.value) => None,
        Some(got) => Some(format!(
            "{target} {} {:?} (got {:?})",
            op_word(a.op),
            a.value,
            got
        )),
    }
}

fn op_word(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "==",
        CompareOp::Ne => "!=",
        CompareOp::Lt => "<",
        CompareOp::Lte => "<=",
        CompareOp::Gt => ">",
        CompareOp::Gte => ">=",
        CompareOp::Contains => "contains",
    }
}

fn down(
    monitor: &Monitor,
    ts: OffsetDateTime,
    started: Instant,
    status_code: Option<i32>,
    msg: &str,
) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status: MonitorStatus::Down,
        latency_ms: Some(ms_i32(started.elapsed())),
        status_code,
        msg: Some(msg.to_string()),
        retries: 0,
        important: false,
    }
}

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
            // Issue the request, SSRF-guarding the target on every hop (the
            // client follows no redirects itself — `send_guarded` re-resolves +
            // re-guards each Location so a redirect can't reach an internal
            // address the initial URL guard wouldn't have caught).
            let hdrs: Vec<(String, String)> = step
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), interpolate(v, &vars)))
                .collect();
            let body = step.body.as_ref().map(|b| interpolate(b, &vars));
            let resp = match send_guarded(
                &client,
                method,
                url,
                &hdrs,
                body.as_deref(),
                monitor.follow_redirect,
            )
            .await
            {
                Ok(r) => r,
                Err(SendErr::Blocked(b)) => return down(monitor, ts, started, last_status, &b),
                Err(SendErr::Timeout) => {
                    return down(monitor, ts, started, last_status, &format!("{label}: request timed out"))
                }
                Err(SendErr::Failed(e)) => {
                    return down(monitor, ts, started, last_status, &format!("{label}: request failed: {e}"))
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
    // Never let reqwest follow redirects on its own — `send_guarded` follows
    // them manually so every hop is re-resolved + SSRF-guarded. Auto-follow
    // would chase a `Location` to an internal address unchecked.
    ClientBuilder::new()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(monitor.ignore_tls)
        .timeout(Duration::from_secs(monitor.timeout_seconds as u64))
        .build()
}

/// Failure modes from [`send_guarded`], mapped to distinct probe-down reasons.
enum SendErr {
    Blocked(String),
    Timeout,
    Failed(String),
}

/// Send `method url` (with headers/body), then follow redirects manually up to
/// 10 hops — SSRF-guarding the resolved host on the initial request and on
/// every `Location` before connecting. This closes the redirect-to-internal
/// SSRF that reqwest's built-in redirect following would allow, since the
/// async DNS guard can't run inside reqwest's synchronous redirect policy.
async fn send_guarded(
    client: &Client,
    method: Method,
    url: String,
    headers: &[(String, String)],
    body: Option<&str>,
    follow_redirect: bool,
) -> Result<reqwest::Response, SendErr> {
    let mut current =
        reqwest::Url::parse(&url).map_err(|e| SendErr::Failed(format!("bad url: {e}")))?;
    let mut method = method;
    let mut body = body.map(str::to_string);

    for _ in 0..=10 {
        if let Some(host) = current.host_str() {
            let port = current.port_or_known_default().unwrap_or(80);
            if let Err(blocked) =
                crate::ssrf::resolve_guarded(host, port, crate::ssrf::block_private_enabled()).await
            {
                return Err(SendErr::Blocked(blocked.to_string()));
            }
        }
        let mut req = client.request(method.clone(), current.clone());
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if let Some(b) = &body {
            req = req.body(b.clone());
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(SendErr::Timeout),
            Err(e) => return Err(SendErr::Failed(e.to_string())),
        };
        if !follow_redirect || !resp.status().is_redirection() {
            return Ok(resp);
        }
        let Some(loc) = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(resp); // redirect with no/invalid Location — hand it back as-is
        };
        let next = current
            .join(loc)
            .map_err(|e| SendErr::Failed(format!("bad redirect target: {e}")))?;
        // Per RFC 7231: 303 (and 301/302 from an unsafe method) downgrade to GET
        // and drop the body; 307/308 preserve method + body.
        let code = resp.status().as_u16();
        if code == 303 || ((code == 301 || code == 302) && method != Method::GET && method != Method::HEAD) {
            method = Method::GET;
            body = None;
        }
        current = next;
    }
    Err(SendErr::Failed("too many redirects".into()))
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

/// Evaluate a list of assertions against a single HTTP response. Returns the
/// first failure reason, or None if all pass. Reused by the HTTP monitor probe
/// so plain monitors get the same status/header/json/body checks as synthetics.
/// `headers` keys must be lowercased. The body is parsed as JSON once (lazily)
/// for any `json` assertions.
pub fn evaluate_assertions(
    status: i32,
    headers: &BTreeMap<String, String>,
    body: &str,
    assertions: &[Assertion],
) -> Option<String> {
    let json_body = serde_json::from_str::<Value>(body).ok();
    assertions
        .iter()
        .find_map(|a| assertion_failure(a, status, headers, &json_body, body))
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

#[cfg(test)]
mod assert_tests {
    use super::evaluate_assertions;
    use rampart_core::synthetic::{AssertKind, Assertion, CompareOp};
    use std::collections::BTreeMap;

    fn mk(kind: AssertKind, path: Option<&str>, op: CompareOp, value: &str) -> Assertion {
        Assertion {
            kind,
            path: path.map(String::from),
            op,
            value: value.into(),
        }
    }

    #[test]
    fn passes_and_fails_across_kinds() {
        let mut h = BTreeMap::new();
        h.insert("content-type".to_string(), "application/json".to_string());
        let body = r#"{"ok":true,"n":5}"#;

        let pass = vec![
            mk(AssertKind::Status, None, CompareOp::Eq, "200"),
            mk(
                AssertKind::Header,
                Some("content-type"),
                CompareOp::Contains,
                "json",
            ),
            mk(AssertKind::Json, Some("ok"), CompareOp::Eq, "true"),
            mk(AssertKind::Json, Some("n"), CompareOp::Gte, "5"),
            mk(AssertKind::BodyContains, None, CompareOp::Eq, "\"n\":5"),
        ];
        assert_eq!(evaluate_assertions(200, &h, body, &pass), None);

        // First failure is reported.
        let fail = vec![mk(AssertKind::Json, Some("n"), CompareOp::Gt, "10")];
        assert!(evaluate_assertions(200, &h, body, &fail).is_some());
        // Missing header → fail.
        let miss = vec![mk(AssertKind::Header, Some("x-nope"), CompareOp::Eq, "1")];
        assert!(evaluate_assertions(200, &h, body, &miss).is_some());
    }
}

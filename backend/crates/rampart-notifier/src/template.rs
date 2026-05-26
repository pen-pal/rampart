//! Minimal `{{placeholder}}` template renderer.
//!
//! Intentionally simpler than Handlebars — we only need flat variable
//! substitution. The cost is no loops / conditionals, the benefit is
//! ~50 lines of zero-dep code that's easy to audit.
//!
//! Supported placeholders:
//!   {{monitor.name}}     monitor display name
//!   {{monitor.url}}      target URL (empty if hostname-based)
//!   {{monitor.kind}}     http / tcp / ping / …
//!   {{monitor.id}}       UUID
//!   {{status}}           current status ("up", "down", "degraded", …)
//!   {{prev_status}}      previous status, or "unknown"
//!   {{latency_ms}}       heartbeat latency, empty if absent
//!   {{status_code}}      HTTP status code if any
//!   {{msg}}              probe-supplied message ("OK", "timed out", …)
//!   {{retries}}          retry count of this heartbeat
//!   {{ts}}               heartbeat timestamp (RFC 3339)

use crate::Event;

pub fn render(template: &str, event: &Event) -> String {
    let lat = event.heartbeat.latency_ms.map(|x| x.to_string()).unwrap_or_default();
    let code = event.heartbeat.status_code.map(|x| x.to_string()).unwrap_or_default();
    let kind = serde_json::to_string(&event.monitor.kind)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();
    let id = event.monitor.id.0.to_string();
    let ts = event.heartbeat.ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let pairs: &[(&str, &str)] = &[
        ("{{monitor.name}}",  &event.monitor.name),
        ("{{monitor.url}}",   event.monitor.url.as_deref().unwrap_or("")),
        ("{{monitor.kind}}",  &kind),
        ("{{monitor.id}}",    &id),
        ("{{status}}",        event.status_str()),
        ("{{prev_status}}",   event.prev_status_str()),
        ("{{latency_ms}}",    &lat),
        ("{{status_code}}",   &code),
        ("{{msg}}",           event.heartbeat.msg.as_deref().unwrap_or("")),
        ("{{retries}}",       &event.heartbeat.retries.to_string()),
        ("{{ts}}",            &ts),
    ];
    let mut out = template.to_string();
    for (k, v) in pairs {
        out = out.replace(k, v);
    }
    out
}

/// Sensible defaults when no template is configured. Used as fallback so
/// users can wire a channel up before bothering with custom templates.
pub fn default_subject(event: &Event) -> String {
    render("[{{status}}] {{monitor.name}}", event)
}

pub fn default_body(event: &Event) -> String {
    let template = r#"{{monitor.name}} is now {{status}} (was {{prev_status}}).

Kind:     {{monitor.kind}}
Target:   {{monitor.url}}
Latency:  {{latency_ms}}ms
Code:     {{status_code}}
Message:  {{msg}}
Time:     {{ts}}
Monitor:  {{monitor.id}}
"#;
    render(template, event)
}

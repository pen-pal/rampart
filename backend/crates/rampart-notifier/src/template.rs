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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventKind};
    use rampart_core::testing::{sample_heartbeat_down, sample_heartbeat_up, sample_monitor};
    use rampart_core::MonitorStatus;

    fn event_down_to_up(_msg: Option<&str>) -> Event {
        let mut m = sample_monitor();
        m.name = "API · production".into();
        let hb = sample_heartbeat_up(&m);
        Event { kind: EventKind::StatusFlip, monitor: m, heartbeat: hb, prev_status: Some(MonitorStatus::Down) }
    }

    fn event_up_to_down() -> Event {
        let m = sample_monitor();
        let hb = sample_heartbeat_down(&m);
        Event { kind: EventKind::StatusFlip, monitor: m, heartbeat: hb, prev_status: Some(MonitorStatus::Up) }
    }

    #[test]
    fn render_substitutes_known_placeholders() {
        let e = event_down_to_up(None);
        let out = render("hi {{monitor.name}} status={{status}} was={{prev_status}}", &e);
        assert_eq!(out, "hi API · production status=up was=down");
    }

    #[test]
    fn render_leaves_unknown_placeholders_alone() {
        let e = event_down_to_up(None);
        let out = render("nope: {{not_real}}", &e);
        assert_eq!(out, "nope: {{not_real}}");
    }

    #[test]
    fn render_substitutes_kind_as_snake_case_string() {
        let e = event_down_to_up(None);
        let out = render("{{monitor.kind}}", &e);
        assert_eq!(out, "http");
    }

    #[test]
    fn render_handles_optional_fields_gracefully() {
        let mut e = event_down_to_up(None);
        // latency + status_code default to Some in the fixture; clear them.
        e.heartbeat.latency_ms  = None;
        e.heartbeat.status_code = None;
        e.heartbeat.msg         = None;
        let out = render("{{latency_ms}}|{{status_code}}|{{msg}}", &e);
        assert_eq!(out, "||");
    }

    #[test]
    fn render_replaces_multiple_occurrences_of_same_placeholder() {
        let e = event_down_to_up(None);
        let out = render("{{status}} {{status}} {{status}}", &e);
        assert_eq!(out, "up up up");
    }

    #[test]
    fn render_ts_is_rfc3339() {
        let e = event_down_to_up(None);
        let out = render("{{ts}}", &e);
        // Loose check — full parsing of OffsetDateTime is overkill here.
        assert!(out.contains('T'),  "ts should be RFC3339, got: {out}");
        assert!(out.ends_with('Z') || out.contains('+') || out.contains('-'),
                "ts should carry an offset, got: {out}");
    }

    #[test]
    fn default_subject_includes_status_and_name() {
        let e = event_up_to_down();
        let s = default_subject(&e);
        assert!(s.contains("down"));
        assert!(s.contains(&e.monitor.name));
    }

    #[test]
    fn default_body_includes_prev_and_msg() {
        let e = event_up_to_down();
        let b = default_body(&e);
        assert!(b.contains("upstream timed out"), "body should include the down msg");
        assert!(b.contains("(was up)"),           "body should show previous status");
        assert!(b.contains("Code:     503"),       "body should include status code");
    }
}

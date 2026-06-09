//! Notification template renderer — Liquid dialect.
//!
//! Standard Liquid surface for notification subject + body. Supports
//! filters (`upcase`, `downcase`, `date`, `truncate`, …), conditionals
//! (`{% if status == "down" %}…{% endif %}`), and loops. Unknown vars
//! render to empty strings, matching Liquid's default lenient mode.
//!
//! Available top-level variables:
//!
//! ```liquid
//! {{ monitor.name }}     — display name
//! {{ monitor.url }}      — target URL (empty if hostname-based)
//! {{ monitor.kind }}     — http / tcp / ping / …
//! {{ monitor.id }}       — UUID
//! {{ monitor.hostname }} — for kinds that use hostname instead of URL
//! {{ monitor.port }}     — when set
//! {{ status }}           — current heartbeat status string
//! {{ prev_status }}      — previous status, or "unknown"
//! {{ latency_ms }}       — int, empty if absent
//! {{ status_code }}      — int, empty if absent
//! {{ msg }}              — probe-supplied message
//! {{ retries }}          — retry count of this heartbeat
//! {{ ts }}               — heartbeat timestamp (RFC 3339)
//! {{ slo_target_pct }}   — configured SLO target (empty if unset)
//! {{ slo_current_pct }}  — current rolling uptime % at event time
//!                          (only set for slo_breached / slo_recovered)
//! ```
//!
//! Backwards compatibility: pre-Liquid templates used flat keys like
//! `{{monitor.name}}` (no spaces). Liquid accepts both `{{monitor.name}}`
//! and `{{ monitor.name }}`, so existing templates render unchanged.

use crate::{Event, EventKind};
use liquid::{object, Object, ParserBuilder};
use once_cell::sync::Lazy;

static PARSER: Lazy<liquid::Parser> = Lazy::new(|| {
    ParserBuilder::with_stdlib()
        .build()
        .expect("liquid parser builds with stdlib filters")
});

pub fn render(template: &str, event: &Event) -> String {
    let tpl = match PARSER.parse(template) {
        Ok(t) => t,
        Err(e) => {
            // Bad template — render the error inline rather than blowing
            // up. The notifier service logs failures separately, but the
            // delivered message making the template visible helps the
            // user diagnose without combing through logs.
            return format!("[template error: {e}]");
        }
    };
    match tpl.render(&build_context(event)) {
        Ok(s) => s,
        Err(e) => format!("[template render error: {e}]"),
    }
}

fn build_context(event: &Event) -> Object {
    let monitor = &event.monitor;
    let hb = &event.heartbeat;

    let kind = serde_json::to_string(&monitor.kind)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();
    let id = monitor.id.0.to_string();
    let ts = hb
        .ts
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    let monitor_obj = object!({
        "name":     monitor.name.clone(),
        "url":      monitor.url.clone().unwrap_or_default(),
        "kind":     kind,
        "id":       id,
        "hostname": monitor.hostname.clone().unwrap_or_default(),
        "port":     monitor.port.unwrap_or_default(),
    });

    // SLO context — only meaningful for SloBreached / SloRecovered, but
    // exposing the placeholders unconditionally keeps the template engine
    // simple. Format to one decimal place to match the dashboard SLO card.
    let slo_target_pct = monitor
        .slo_target_pct
        .map(|v| format!("{v:.1}"))
        .unwrap_or_default();
    let slo_current_pct = event
        .slo_current_pct
        .map(|v| format!("{v:.2}"))
        .unwrap_or_default();

    object!({
        "monitor":         monitor_obj,
        "status":          event.status_str(),
        "prev_status":     event.prev_status_str(),
        "latency_ms":      hb.latency_ms.map(|x| x.to_string()).unwrap_or_default(),
        "status_code":     hb.status_code.map(|x| x.to_string()).unwrap_or_default(),
        "msg":             hb.msg.clone().unwrap_or_default(),
        "retries":         hb.retries,
        "ts":              ts,
        "slo_target_pct":  slo_target_pct,
        "slo_current_pct": slo_current_pct,
    })
}

/// Sensible defaults when no template is configured. Used as fallback so
/// users can wire a channel up before bothering with custom templates.
///
/// Branches on `event.kind`: SLO breach / recovery events get their own
/// short subject + body that mention the target and current %; everything
/// else falls back to the long-standing status-flip default.
pub fn default_subject(event: &Event) -> String {
    match event.kind {
        EventKind::SloBreached => render(
            "[SLO breach] {{ monitor.name }} — {{ slo_current_pct }}% < {{ slo_target_pct }}%",
            event,
        ),
        EventKind::SloRecovered => render(
            "[SLO recovered] {{ monitor.name }} — {{ slo_current_pct }}% ≥ {{ slo_target_pct }}%",
            event,
        ),
        EventKind::MaintenanceStarted => {
            render("{{ monitor.name }}: scheduled maintenance started", event)
        }
        EventKind::MaintenanceEnded => {
            render("{{ monitor.name }}: scheduled maintenance ended", event)
        }
        _ => render("[{{ status }}] {{ monitor.name }}", event),
    }
}

pub fn default_body(event: &Event) -> String {
    match event.kind {
        EventKind::SloBreached => render(
            "{{ monitor.name }} SLO breached — current {{ slo_current_pct }}% < target {{ slo_target_pct }}%.\nMonitor: {{ monitor.id }}\nTime:    {{ ts }}\n",
            event,
        ),
        EventKind::SloRecovered => render(
            "{{ monitor.name }} SLO recovered — current {{ slo_current_pct }}% ≥ target {{ slo_target_pct }}%.\nMonitor: {{ monitor.id }}\nTime:    {{ ts }}\n",
            event,
        ),
        EventKind::MaintenanceStarted => render(
            "{{ monitor.name }}: scheduled maintenance started.\nMonitor: {{ monitor.id }}\nTime:    {{ ts }}\n",
            event,
        ),
        EventKind::MaintenanceEnded => render(
            "{{ monitor.name }}: scheduled maintenance ended.\nMonitor: {{ monitor.id }}\nTime:    {{ ts }}\n",
            event,
        ),
        _ => {
            let template = r#"{{ monitor.name }} is now {{ status }} (was {{ prev_status }}).

Kind:     {{ monitor.kind }}
Target:   {{ monitor.url }}
Latency:  {{ latency_ms }}ms
Code:     {{ status_code }}
Message:  {{ msg }}
Time:     {{ ts }}
Monitor:  {{ monitor.id }}
"#;
            render(template, event)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventKind};
    use rampart_core::testing::{sample_heartbeat_down, sample_heartbeat_up, sample_monitor};
    use rampart_core::MonitorStatus;

    fn event_down_to_up() -> Event {
        let mut m = sample_monitor();
        m.name = "API · production".into();
        let hb = sample_heartbeat_up(&m);
        Event {
            kind: EventKind::StatusFlip,
            monitor: m,
            heartbeat: hb,
            prev_status: Some(MonitorStatus::Down),
            slo_current_pct: None,
        }
    }

    fn event_up_to_down() -> Event {
        let m = sample_monitor();
        let hb = sample_heartbeat_down(&m);
        Event {
            kind: EventKind::StatusFlip,
            monitor: m,
            heartbeat: hb,
            prev_status: Some(MonitorStatus::Up),
            slo_current_pct: None,
        }
    }

    #[test]
    fn render_substitutes_known_placeholders() {
        let e = event_down_to_up();
        let out = render(
            "hi {{ monitor.name }} status={{ status }} was={{ prev_status }}",
            &e,
        );
        assert_eq!(out, "hi API · production status=up was=down");
    }

    #[test]
    fn render_legacy_no_space_form_still_works() {
        // The pre-Liquid templates used `{{monitor.name}}` with no spaces.
        // Liquid accepts that too, so stored user templates render the same.
        let e = event_down_to_up();
        let out = render("{{monitor.name}}/{{status}}", &e);
        assert_eq!(out, "API · production/up");
    }

    #[test]
    fn render_supports_liquid_filters() {
        let e = event_up_to_down();
        let out = render("{{ status | upcase }}", &e);
        assert_eq!(out, "DOWN");
    }

    #[test]
    fn render_supports_conditional_blocks() {
        let e = event_up_to_down();
        let out = render(r#"{% if status == "down" %}🔴{% else %}🟢{% endif %}"#, &e);
        assert_eq!(out, "🔴");
    }

    #[test]
    fn render_reports_unknown_var_inline() {
        // Liquid is strict by default — a typo'd variable surfaces as
        // an inline render error rather than silently dropping. Users
        // who want empty-on-missing can write `{{ foo | default: "" }}`.
        let e = event_down_to_up();
        let out = render("nope: {{ not_real }}", &e);
        assert!(out.starts_with("[template render error"), "got: {out}");
    }

    #[test]
    fn render_supports_default_filter_for_known_empty_var() {
        // `default:` kicks in when the variable resolves but is empty.
        let mut e = event_down_to_up();
        e.heartbeat.msg = None;
        let out = render(r#"{{ msg | default: "n/a" }}"#, &e);
        assert_eq!(out, "n/a");
    }

    #[test]
    fn render_substitutes_kind_as_snake_case_string() {
        let e = event_down_to_up();
        let out = render("{{ monitor.kind }}", &e);
        assert_eq!(out, "http");
    }

    #[test]
    fn render_handles_optional_fields_gracefully() {
        let mut e = event_down_to_up();
        e.heartbeat.latency_ms = None;
        e.heartbeat.status_code = None;
        e.heartbeat.msg = None;
        let out = render("{{ latency_ms }}|{{ status_code }}|{{ msg }}", &e);
        assert_eq!(out, "||");
    }

    #[test]
    fn render_replaces_multiple_occurrences_of_same_placeholder() {
        let e = event_down_to_up();
        let out = render("{{ status }} {{ status }} {{ status }}", &e);
        assert_eq!(out, "up up up");
    }

    #[test]
    fn render_bad_template_inlines_error_instead_of_panicking() {
        let e = event_down_to_up();
        let out = render("{% if %}", &e);
        assert!(out.starts_with("[template error"), "got: {out}");
    }

    #[test]
    fn render_ts_is_rfc3339() {
        let e = event_down_to_up();
        let out = render("{{ ts }}", &e);
        assert!(out.contains('T'), "ts should be RFC3339, got: {out}");
        assert!(
            out.ends_with('Z') || out.contains('+') || out.contains('-'),
            "ts should carry an offset, got: {out}"
        );
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
        assert!(b.contains("upstream timed out"));
        assert!(b.contains("(was up)"));
        assert!(b.contains("Code:     503"));
    }

    fn slo_event(kind: EventKind) -> Event {
        let mut m = sample_monitor();
        m.name = "API · production".into();
        m.slo_target_pct = Some(99.9);
        let hb = sample_heartbeat_up(&m);
        Event {
            kind,
            monitor: m,
            heartbeat: hb,
            prev_status: Some(MonitorStatus::Up),
            slo_current_pct: Some(99.42),
        }
    }

    #[test]
    fn default_subject_slo_breached_mentions_target_and_current() {
        let s = default_subject(&slo_event(EventKind::SloBreached));
        assert!(s.contains("SLO breach"), "got: {s}");
        assert!(s.contains("99.42"), "got: {s}");
        assert!(s.contains("99.9"), "got: {s}");
    }

    #[test]
    fn default_body_slo_breached_has_breach_phrasing() {
        let b = default_body(&slo_event(EventKind::SloBreached));
        assert!(b.contains("SLO breached"), "got: {b}");
        assert!(b.contains("current 99.42%"), "got: {b}");
        assert!(b.contains("target 99.9%"), "got: {b}");
    }

    #[test]
    fn default_subject_slo_recovered_mentions_recovered() {
        let s = default_subject(&slo_event(EventKind::SloRecovered));
        assert!(s.contains("SLO recovered"), "got: {s}");
        assert!(s.contains("99.9"), "got: {s}");
    }

    #[test]
    fn render_slo_placeholders_empty_when_event_lacks_slo_context() {
        // Non-SLO events still tolerate `{{ slo_current_pct }}` in custom
        // templates — they just render to empty strings.
        let e = event_up_to_down();
        let out = render("[{{ slo_target_pct }}|{{ slo_current_pct }}]", &e);
        assert_eq!(out, "[|]");
    }

    fn maintenance_event(kind: EventKind) -> Event {
        let mut m = sample_monitor();
        m.name = "API · production".into();
        let hb = sample_heartbeat_up(&m);
        Event {
            kind,
            monitor: m,
            heartbeat: hb,
            prev_status: None,
            slo_current_pct: None,
        }
    }

    #[test]
    fn default_subject_maintenance_started() {
        let s = default_subject(&maintenance_event(EventKind::MaintenanceStarted));
        assert_eq!(s, "API · production: scheduled maintenance started");
    }

    #[test]
    fn default_subject_maintenance_ended() {
        let s = default_subject(&maintenance_event(EventKind::MaintenanceEnded));
        assert_eq!(s, "API · production: scheduled maintenance ended");
    }

    #[test]
    fn default_body_maintenance_started_mentions_monitor() {
        let b = default_body(&maintenance_event(EventKind::MaintenanceStarted));
        assert!(b.contains("scheduled maintenance started"), "got: {b}");
        assert!(b.contains("API · production"), "got: {b}");
    }
}

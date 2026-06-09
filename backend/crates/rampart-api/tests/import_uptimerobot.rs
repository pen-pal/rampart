//! Integration test for the UptimeRobot importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the JSON fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running the importer. Keeping it here keeps the round-
//! trip honest: change the fixture or the mapping and the test fails.
//!
//! No DB needed — the dry-run path is the contract that matters; the
//! existing `tests/monitors.rs` already exercises the
//! `monitors::create` repository helper.

use rampart_api::importers::{uptimerobot, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/uptimerobot-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = uptimerobot::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 entries total in the fixture; one is type 99 which has no
    // Rampart equivalent and should land in `skipped`, not `mapped`.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");
    assert_eq!(plan.skipped[0].source_kind, "99");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Keyword), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
    assert_eq!(count(&plan, MonitorKind::Smtp), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = uptimerobot::parse_and_map(FIXTURE).expect("fixture parses");

    // HTTP: `url` -> `url`, friendly_name -> name, interval/timeout copied.
    let http = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Http)
        .expect("http entry present");
    assert_eq!(http.new_monitor.name, "Acme HTTP");
    assert_eq!(
        http.new_monitor.url.as_deref(),
        Some("https://api.example.com/health")
    );
    assert_eq!(http.new_monitor.interval_seconds, 60);
    assert_eq!(http.new_monitor.timeout_seconds, 10);
    // Default HTTP method when the source has none.
    assert_eq!(http.new_monitor.http_method, "GET");

    // Keyword: `keyword_value` stashed under `config["keyword"]`.
    let kw = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Keyword)
        .expect("keyword entry present");
    assert_eq!(kw.new_monitor.name, "Acme HTTP w/ keyword");
    assert_eq!(
        kw.new_monitor.url.as_deref(),
        Some("https://app.example.com")
    );
    assert_eq!(kw.new_monitor.config["keyword"].as_str(), Some("OK"));
    assert_eq!(kw.new_monitor.interval_seconds, 120);

    // Ping: UptimeRobot stores the ICMP target in `url`; importer moves it
    // to `hostname` because Rampart's Ping probe addresses by host.
    let ping = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Ping)
        .expect("ping entry present");
    assert_eq!(ping.new_monitor.name, "Edge router ping");
    assert_eq!(ping.new_monitor.hostname.as_deref(), Some("10.0.0.1"));
    assert!(ping.new_monitor.url.is_none());
    assert_eq!(ping.new_monitor.interval_seconds, 300);

    // SMTP (type 4 / sub_type 4): host from `url`, explicit `port`.
    let smtp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Smtp)
        .expect("smtp entry present");
    assert_eq!(smtp.new_monitor.name, "Acme SMTP relay");
    assert_eq!(
        smtp.new_monitor.hostname.as_deref(),
        Some("smtp.internal.example.com")
    );
    assert_eq!(smtp.new_monitor.port, Some(25));
    assert_eq!(smtp.new_monitor.interval_seconds, 300);
    assert_eq!(smtp.new_monitor.timeout_seconds, 15);
}

//! Integration test for the StatusCake importer's parse + map pass.
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

use rampart_api::importers::{statuscake, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/statuscake-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = statuscake::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 entries total in the fixture; all map (StatusCake has direct
    // Rampart equivalents for HTTP, PING, TCP, DNS, SMTP).
    assert_eq!(plan.mapped.len(), 5, "mapped count");
    assert!(plan.skipped.is_empty(), "skipped should be empty");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Dns), 1);
    assert_eq!(count(&plan, MonitorKind::Smtp), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = statuscake::parse_and_map(FIXTURE).expect("fixture parses");

    // HTTP entry: name -> Monitor.name, website_url -> url,
    // check_rate -> interval_seconds, timeout -> timeout_seconds.
    let http = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Http)
        .expect("http entry present");
    assert_eq!(http.new_monitor.name, "Acme API HTTP");
    assert_eq!(
        http.new_monitor.url.as_deref(),
        Some("https://api.example.com/health")
    );
    assert_eq!(http.new_monitor.interval_seconds, 60);
    assert_eq!(http.new_monitor.timeout_seconds, 15);

    // Ping: website_url carries the bare host.
    let ping = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Ping)
        .expect("ping entry present");
    assert_eq!(ping.new_monitor.name, "Acme ping");
    assert_eq!(ping.new_monitor.url.as_deref(), Some("10.0.0.1"));
    assert_eq!(ping.new_monitor.interval_seconds, 300);
    assert_eq!(ping.new_monitor.timeout_seconds, 10);

    // TCP entry.
    let tcp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Tcp)
        .expect("tcp entry present");
    assert_eq!(tcp.new_monitor.name, "Acme DB tcp");
    assert_eq!(tcp.new_monitor.interval_seconds, 120);
    assert_eq!(tcp.new_monitor.timeout_seconds, 12);

    // DNS entry.
    let dns = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Dns)
        .expect("dns entry present");
    assert_eq!(dns.new_monitor.name, "Acme DNS");
    assert_eq!(dns.new_monitor.url.as_deref(), Some("example.com"));

    // SMTP entry.
    let smtp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Smtp)
        .expect("smtp entry present");
    assert_eq!(smtp.new_monitor.name, "Acme SMTP relay");
    assert_eq!(smtp.new_monitor.interval_seconds, 600);
    assert_eq!(smtp.new_monitor.timeout_seconds, 20);
}

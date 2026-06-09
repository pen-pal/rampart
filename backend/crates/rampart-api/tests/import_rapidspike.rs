//! Integration test for the RapidSpike importer's parse + map pass.
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

use rampart_api::importers::{rapidspike, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/rapidspike-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = rapidspike::parse_and_map(FIXTURE).expect("fixture parses");

    // 4 entries total in the fixture; all map (RapidSpike has direct
    // Rampart equivalents for http_check, tcp_check, dns_check, ping_check).
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert!(plan.skipped.is_empty(), "skipped should be empty");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Dns), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = rapidspike::parse_and_map(FIXTURE).expect("fixture parses");

    // HTTP entry: name -> Monitor.name, url -> url,
    // check_interval -> interval_seconds, timeout_seconds -> timeout_seconds.
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
    assert_eq!(http.new_monitor.timeout_seconds, 15);
    assert_eq!(http.new_monitor.http_method, "GET");

    // TCP entry.
    let tcp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Tcp)
        .expect("tcp entry present");
    assert_eq!(tcp.new_monitor.name, "Acme DB");
    assert_eq!(
        tcp.new_monitor.url.as_deref(),
        Some("db.internal.example.com")
    );
    assert_eq!(tcp.new_monitor.interval_seconds, 120);
    assert_eq!(tcp.new_monitor.timeout_seconds, 10);

    // DNS entry.
    let dns = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Dns)
        .expect("dns entry present");
    assert_eq!(dns.new_monitor.name, "Acme DNS");
    assert_eq!(dns.new_monitor.url.as_deref(), Some("example.com"));
    assert_eq!(dns.new_monitor.interval_seconds, 300);

    // Ping entry.
    let ping = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Ping)
        .expect("ping entry present");
    assert_eq!(ping.new_monitor.name, "Acme Ping");
    assert_eq!(ping.new_monitor.url.as_deref(), Some("10.0.0.1"));
    assert_eq!(ping.new_monitor.interval_seconds, 300);
}

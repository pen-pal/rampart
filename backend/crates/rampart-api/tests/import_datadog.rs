//! Integration test for the Datadog Synthetics importer's parse + map
//! pass.
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

use rampart_api::importers::{datadog, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/datadog-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = datadog::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 entries total in the fixture; one is `(api, multi)` which
    // Rampart does not model and should land in `skipped`, not
    // `mapped`.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");
    assert_eq!(plan.skipped[0].source_kind, "api/multi");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Keyword), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = datadog::parse_and_map(FIXTURE).expect("fixture parses");

    let tcp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Tcp)
        .expect("tcp entry present");
    assert_eq!(tcp.new_monitor.name, "Acme primary DB");
    assert_eq!(
        tcp.new_monitor.hostname.as_deref(),
        Some("db.internal.example.com")
    );
    assert_eq!(tcp.new_monitor.port, Some(5432));
    assert_eq!(tcp.new_monitor.interval_seconds, 300);
    assert_eq!(tcp.new_monitor.timeout_seconds, 15);
    assert_eq!(tcp.new_monitor.max_retries, 2);

    let http = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Http)
        .expect("http entry present");
    assert_eq!(
        http.new_monitor.url.as_deref(),
        Some("https://api.example.com/health")
    );
    assert_eq!(http.new_monitor.http_method, "GET");
    assert_eq!(http.new_monitor.timeout_seconds, 10);

    let kw = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Keyword)
        .expect("keyword entry present");
    assert_eq!(
        kw.new_monitor.url.as_deref(),
        Some("https://app.example.com")
    );
    assert_eq!(kw.new_monitor.interval_seconds, 120);
    assert_eq!(kw.new_monitor.max_retries, 1);

    let ping = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Ping)
        .expect("ping entry present");
    assert_eq!(ping.new_monitor.hostname.as_deref(), Some("10.0.0.1"));
    // No tick_every override? Actually fixture sets 60 — defaults apply
    // identically. Asserting on the resolved value is what matters.
    assert_eq!(ping.new_monitor.interval_seconds, 60);
}

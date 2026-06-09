//! Integration test for the Checkly importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the JSON fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running the importer.
//!
//! No DB needed — the dry-run path is the contract that matters.

use rampart_api::importers::{checkly, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/checkly-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = checkly::parse_and_map(FIXTURE).expect("fixture parses");

    // 4 checks: API -> Http, BROWSER -> Browser, TCP -> Tcp map; the
    // HEARTBEAT check has no Rampart equivalent and is skipped.
    assert_eq!(plan.mapped.len(), 3, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");
    assert_eq!(plan.skipped[0].source_kind, "HEARTBEAT");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Browser), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = checkly::parse_and_map(FIXTURE).expect("fixture parses");

    // API: request.url -> url; request.method -> http_method;
    // frequency (minutes) -> interval_seconds.
    let site = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Marketing site")
        .expect("Marketing site present");
    assert_eq!(site.mapped_kind, MonitorKind::Http);
    assert_eq!(
        site.new_monitor.url.as_deref(),
        Some("https://www.example.com")
    );
    assert!(site.new_monitor.hostname.is_none());
    assert_eq!(site.new_monitor.http_method, "GET");
    assert_eq!(site.new_monitor.interval_seconds, 60, "1 minute -> 60s");

    // BROWSER -> Browser; 5 minutes -> 300s.
    let journey = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Checkout journey")
        .expect("Checkout journey present");
    assert_eq!(journey.mapped_kind, MonitorKind::Browser);
    assert_eq!(
        journey.new_monitor.interval_seconds, 300,
        "5 minutes -> 300s"
    );

    // TCP: tcp.host -> hostname; tcp.port -> port.
    let db = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Primary database")
        .expect("Primary database present");
    assert_eq!(db.mapped_kind, MonitorKind::Tcp);
    assert!(db.new_monitor.url.is_none());
    assert_eq!(db.new_monitor.hostname.as_deref(), Some("db.example.com"));
    assert_eq!(db.new_monitor.port, Some(5432));
    assert_eq!(db.new_monitor.interval_seconds, 60, "1 minute -> 60s");
}

//! Integration test for the Pingometer importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the JSON fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running the importer.
//!
//! No DB needed — the dry-run path is the contract that matters.

use rampart_api::importers::{pingometer, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/pingometer-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = pingometer::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 monitors: 2 http -> Http, 1 ping -> Ping, 1 tcp -> Tcp,
    // 1 transaction -> skipped (no Rampart equivalent).
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");

    assert_eq!(count(&plan, MonitorKind::Http), 2);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);

    assert_eq!(plan.skipped[0].source_kind, "transaction");
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = pingometer::parse_and_map(FIXTURE).expect("fixture parses");

    // http: url -> url; interval MINUTES (5) -> 300 seconds.
    let api = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "API gateway")
        .expect("API gateway present");
    assert_eq!(api.mapped_kind, MonitorKind::Http);
    assert_eq!(
        api.new_monitor.url.as_deref(),
        Some("https://api.example.com/health")
    );
    assert!(api.new_monitor.hostname.is_none());
    assert_eq!(api.new_monitor.interval_seconds, 300);

    // tcp: url -> hostname; port -> port; interval 2min -> 120s.
    let db = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Primary database")
        .expect("Primary database present");
    assert_eq!(db.mapped_kind, MonitorKind::Tcp);
    assert!(db.new_monitor.url.is_none());
    assert_eq!(db.new_monitor.hostname.as_deref(), Some("db.example.com"));
    assert_eq!(db.new_monitor.port, Some(5432));
    assert_eq!(db.new_monitor.interval_seconds, 120);

    // ping: url -> hostname; interval 1min floored at 60s.
    let router = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Edge router")
        .expect("Edge router present");
    assert_eq!(router.mapped_kind, MonitorKind::Ping);
    assert_eq!(router.new_monitor.hostname.as_deref(), Some("10.0.0.1"));
    assert!(router.new_monitor.url.is_none());
    assert_eq!(router.new_monitor.interval_seconds, 60);
}

//! Integration test for the generic CSV importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the CSV fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running `rampart-import csv`.
//!
//! Unlike the JSON importers, CSV has its own entry point
//! (`parse_csv_and_map`) because the input is raw CSV text, not JSON.
//!
//! No DB needed — the dry-run path is the contract that matters.

use rampart_api::importers::{csv_import, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/csv-sample.csv");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = csv_import::parse_csv_and_map(FIXTURE).expect("fixture parses");

    // 5 data rows: http + tcp + ping + dns map; the `frobnicate` row has
    // an unknown kind and is skipped.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
    assert_eq!(count(&plan, MonitorKind::Dns), 1);

    assert_eq!(plan.skipped[0].source_kind, "frobnicate");
    assert!(plan.skipped[0].reason.contains("unknown kind"));
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = csv_import::parse_csv_and_map(FIXTURE).expect("fixture parses");

    // http: url column -> url; intervals/timeouts parsed.
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
    assert_eq!(site.new_monitor.interval_seconds, 60);
    assert_eq!(site.new_monitor.timeout_seconds, 16);

    // tcp: hostname + port columns; custom interval/timeout.
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
    assert_eq!(db.new_monitor.timeout_seconds, 10);

    // dns: hostname column; custom interval.
    let resolver = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Public resolver")
        .expect("Public resolver present");
    assert_eq!(resolver.mapped_kind, MonitorKind::Dns);
    assert_eq!(
        resolver.new_monitor.hostname.as_deref(),
        Some("example.com")
    );
    assert!(resolver.new_monitor.url.is_none());
    assert_eq!(resolver.new_monitor.interval_seconds, 300);
}

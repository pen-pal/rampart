//! Integration test for the Cachet importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the JSON fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running the importer.
//!
//! No DB needed — the dry-run path is the contract that matters.

use rampart_api::importers::{cachet, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/cachet-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = cachet::parse_and_map(FIXTURE).expect("fixture parses");

    // 4 components: 2 have http(s) links -> Http; one has an empty link
    // and one has a non-http (ftp) link -> both skipped (no probe
    // target — Cachet components can be manual-only).
    assert_eq!(plan.mapped.len(), 2, "mapped count");
    assert_eq!(plan.skipped.len(), 2, "skipped count");

    assert_eq!(count(&plan, MonitorKind::Http), 2);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = cachet::parse_and_map(FIXTURE).expect("fixture parses");

    let api = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "API gateway")
        .expect("API gateway present");
    assert_eq!(api.mapped_kind, MonitorKind::Http);
    assert_eq!(
        api.new_monitor.url.as_deref(),
        Some("https://api.example.com/health"),
        "link -> url"
    );
    // Cachet exports carry no probe cadence; importer defaults to 60s.
    assert_eq!(api.new_monitor.interval_seconds, 60);
    assert_eq!(api.new_monitor.http_method, "GET");
    assert!(api.new_monitor.hostname.is_none());
    assert!(api.new_monitor.port.is_none());

    let site = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Marketing site")
        .expect("Marketing site present");
    assert_eq!(
        site.new_monitor.url.as_deref(),
        Some("https://www.example.com")
    );

    // The empty-link and ftp-link components are the skipped ones.
    assert!(plan
        .skipped
        .iter()
        .any(|s| s.source_name == "Customer database"));
    assert!(plan
        .skipped
        .iter()
        .any(|s| s.source_name == "Legacy FTP drop"));
}

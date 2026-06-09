//! Integration test for the StatusGator importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the JSON fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running the importer.
//!
//! No DB needed — the dry-run path is the contract that matters.

use rampart_api::importers::{statusgator, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/statusgator-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = statusgator::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 services: 4 have a usable url/home_page_url -> Http; the 5th
    // (manual service, no url at all) is skipped.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");

    // StatusGator only tracks web products — everything maps to Http.
    assert_eq!(count(&plan, MonitorKind::Http), 4);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = statusgator::parse_and_map(FIXTURE).expect("fixture parses");

    // Prefers the status-page `url` when present.
    let gh = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "GitHub")
        .expect("GitHub present");
    assert_eq!(gh.mapped_kind, MonitorKind::Http);
    assert_eq!(
        gh.new_monitor.url.as_deref(),
        Some("https://www.githubstatus.com")
    );
    assert!(gh.new_monitor.hostname.is_none());
    // StatusGator carries no cadence; the importer seeds the 300s default.
    assert_eq!(gh.new_monitor.interval_seconds, 300);

    // Falls back to home_page_url when `url` is absent.
    let acme = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Acme Internal")
        .expect("Acme Internal present");
    assert_eq!(
        acme.new_monitor.url.as_deref(),
        Some("https://acme.example.com")
    );

    // The url-less service is the skipped one.
    assert_eq!(plan.skipped[0].source_name, "Manual Service");
}

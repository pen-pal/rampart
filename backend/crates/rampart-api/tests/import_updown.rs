//! Integration test for the Updown.io importer's parse + map pass.
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

use rampart_api::importers::{updown, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/updown-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = updown::parse_and_map(FIXTURE).expect("fixture parses");

    // 4 entries total; Updown is HTTP-only so 3 map to Http and the
    // one with a string_match promotes to Keyword. Nothing is skipped.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert!(plan.skipped.is_empty(), "skipped should be empty");

    assert_eq!(count(&plan, MonitorKind::Http), 3);
    assert_eq!(count(&plan, MonitorKind::Keyword), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = updown::parse_and_map(FIXTURE).expect("fixture parses");

    // Keyword entry: string_match -> config["keyword"], promoted to
    // Keyword kind.
    let kw = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Keyword)
        .expect("keyword entry present");
    assert_eq!(kw.new_monitor.name, "Acme app keyword");
    assert_eq!(
        kw.new_monitor.url.as_deref(),
        Some("https://app.example.com")
    );
    assert_eq!(kw.new_monitor.config["keyword"].as_str(), Some("Welcome"));
    assert_eq!(kw.new_monitor.interval_seconds, 120);
    assert_eq!(kw.new_monitor.http_method, "GET");

    // First HTTP entry: alias -> name, url, period -> interval, GET.
    let api = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Acme API")
        .expect("Acme API present");
    assert_eq!(api.mapped_kind, MonitorKind::Http);
    assert_eq!(
        api.new_monitor.url.as_deref(),
        Some("https://api.example.com/health")
    );
    assert_eq!(api.new_monitor.interval_seconds, 60);
    assert_eq!(api.new_monitor.http_method, "GET");

    // Entry with empty alias: name falls back to url.
    let head_check = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.http_method == "HEAD")
        .expect("HEAD-verb entry present");
    assert_eq!(head_check.mapped_kind, MonitorKind::Http);
    assert_eq!(
        head_check.new_monitor.name, "https://www.example.com",
        "empty alias should fall back to url"
    );
    assert_eq!(head_check.new_monitor.interval_seconds, 300);

    // POST verb is propagated and uppercased.
    let post_check = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Acme POST probe")
        .expect("POST probe present");
    assert_eq!(post_check.new_monitor.http_method, "POST");
    assert_eq!(post_check.new_monitor.interval_seconds, 600);
}

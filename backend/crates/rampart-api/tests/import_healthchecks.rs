//! Integration test for the Healthchecks.io importer's parse + map
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

use rampart_api::importers::{healthchecks, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/healthchecks-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = healthchecks::parse_and_map(FIXTURE).expect("fixture parses");

    // 4 entries total in the fixture; Healthchecks is heartbeat-only so
    // every check maps onto `Push` and nothing lands in `skipped`.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert!(plan.skipped.is_empty(), "skipped should be empty");

    assert_eq!(count(&plan, MonitorKind::Push), 4);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = healthchecks::parse_and_map(FIXTURE).expect("fixture parses");

    // First entry: cron-kind, 24h timeout / 5-min grace -> interval
    // 86400 / timeout 300, schedule + tz + uuid preserved in config.
    let backup = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Nightly DB backup")
        .expect("nightly DB backup present");
    assert_eq!(backup.mapped_kind, MonitorKind::Push);
    assert_eq!(backup.new_monitor.interval_seconds, 86400);
    assert_eq!(backup.new_monitor.timeout_seconds, 300);
    assert_eq!(
        backup.new_monitor.config["healthchecks_uuid"].as_str(),
        Some("aaaa-1111")
    );
    assert_eq!(
        backup.new_monitor.config["healthchecks_kind"].as_str(),
        Some("cron")
    );
    assert_eq!(
        backup.new_monitor.config["schedule"].as_str(),
        Some("0 2 * * *")
    );
    assert_eq!(backup.new_monitor.config["tz"].as_str(), Some("UTC"));

    // Simple-kind heartbeat: timeout 300 -> interval 300, grace 60 ->
    // timeout 60.
    let ingest = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Heartbeat: ingest worker")
        .expect("ingest heartbeat present");
    assert_eq!(ingest.new_monitor.interval_seconds, 300);
    assert_eq!(ingest.new_monitor.timeout_seconds, 60);
    assert_eq!(
        ingest.new_monitor.config["healthchecks_kind"].as_str(),
        Some("simple")
    );

    // oncalendar-kind: schedule preserved verbatim ("hourly").
    let sync = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Hourly sync job")
        .expect("hourly sync present");
    assert_eq!(
        sync.new_monitor.config["healthchecks_kind"].as_str(),
        Some("oncalendar")
    );
    assert_eq!(sync.new_monitor.config["schedule"].as_str(), Some("hourly"));

    // Every entry: no URL / hostname / port — Push doesn't address
    // outbound; Rampart issues the inbound webhook URL itself.
    for m in &plan.mapped {
        assert!(m.new_monitor.url.is_none());
        assert!(m.new_monitor.hostname.is_none());
        assert!(m.new_monitor.port.is_none());
    }
}

//! Integration test for the Cronitor importer's parse + map pass.
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

use rampart_api::importers::{cronitor, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/cronitor-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = cronitor::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 entries total in the fixture; one is the `synthetic` future-
    // type which has no Rampart equivalent and lands in `skipped`.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");
    assert_eq!(plan.skipped[0].source_kind, "synthetic");

    // heartbeat + job -> Push (2 entries); check + uptime -> Http
    // (2 entries).
    assert_eq!(count(&plan, MonitorKind::Push), 2);
    assert_eq!(count(&plan, MonitorKind::Http), 2);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = cronitor::parse_and_map(FIXTURE).expect("fixture parses");

    // Heartbeat -> Push; code carried in config; URL absent (Push).
    let hb = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Nightly job heartbeat")
        .expect("heartbeat entry present");
    assert_eq!(hb.mapped_kind, MonitorKind::Push);
    assert_eq!(
        hb.new_monitor.config["cronitor_code"].as_str(),
        Some("ABCD12")
    );
    assert!(hb.new_monitor.url.is_none());
    // "every 24 hours" -> 86400, within validation range.
    assert_eq!(hb.new_monitor.interval_seconds, 86400);

    // Cron-shaped job: schedule isn't an "every N units" form so
    // interval falls back to the 60s default, but the cron string is
    // preserved in config so the operator can re-create it by hand.
    let job = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Cron backup runner")
        .expect("job entry present");
    assert_eq!(job.mapped_kind, MonitorKind::Push);
    assert_eq!(job.new_monitor.interval_seconds, 60);
    assert_eq!(
        job.new_monitor.config["schedule"].as_str(),
        Some("0 2 * * *")
    );

    // check -> Http; URL + method preserved; "every 60 seconds" -> 60.
    let check = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Acme API check")
        .expect("check entry present");
    assert_eq!(check.mapped_kind, MonitorKind::Http);
    assert_eq!(
        check.new_monitor.url.as_deref(),
        Some("https://api.example.com/health")
    );
    assert_eq!(check.new_monitor.http_method, "GET");
    assert_eq!(check.new_monitor.interval_seconds, 60);
    assert!(check.new_monitor.config["assertions"].is_array());

    // uptime -> Http; "every 5 minutes" -> 300.
    let uptime = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Acme homepage uptime")
        .expect("uptime entry present");
    assert_eq!(uptime.mapped_kind, MonitorKind::Http);
    assert_eq!(
        uptime.new_monitor.url.as_deref(),
        Some("https://www.example.com")
    );
    assert_eq!(uptime.new_monitor.interval_seconds, 300);
}

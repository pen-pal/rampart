//! Integration test for the BetterStack importer's parse + map pass.
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

use rampart_api::importers::{betterstack, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/betterstack-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = betterstack::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 entries total in the fixture; all map (BetterStack has direct
    // Rampart equivalents for each of `status`, `keyword`, `tcp`,
    // `smtp`, `playwright`).
    assert_eq!(plan.mapped.len(), 5, "mapped count");
    assert!(plan.skipped.is_empty(), "skipped should be empty");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Keyword), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Smtp), 1);
    assert_eq!(count(&plan, MonitorKind::Browser), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = betterstack::parse_and_map(FIXTURE).expect("fixture parses");

    // HTTP entry: status -> Http, check_frequency -> interval_seconds,
    // request_timeout -> timeout_seconds, expected_status_codes copied.
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
    assert_eq!(http.new_monitor.accepted_statuses, vec![200, 204]);
    assert_eq!(http.new_monitor.http_method, "GET");

    // Keyword: required_keyword stashed under `config["keyword"]`.
    let kw = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Keyword)
        .expect("keyword entry present");
    assert_eq!(kw.new_monitor.name, "Acme keyword check");
    assert_eq!(
        kw.new_monitor.url.as_deref(),
        Some("https://app.example.com")
    );
    assert_eq!(kw.new_monitor.config["keyword"].as_str(), Some("Welcome"));
    assert_eq!(kw.new_monitor.interval_seconds, 120);

    // TCP: port carried, no url-shaped keyword config.
    let tcp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Tcp)
        .expect("tcp entry present");
    assert_eq!(tcp.new_monitor.name, "Acme primary DB");
    assert_eq!(tcp.new_monitor.port, Some(5432));
    assert_eq!(tcp.new_monitor.interval_seconds, 300);

    // SMTP: port carried, smtp kind.
    let smtp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Smtp)
        .expect("smtp entry present");
    assert_eq!(smtp.new_monitor.port, Some(25));
    assert_eq!(smtp.new_monitor.interval_seconds, 600);

    // Browser: playwright -> Browser; verify_ssl: false -> ignore_tls.
    let browser = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Browser)
        .expect("browser entry present");
    assert_eq!(browser.new_monitor.name, "Acme login flow");
    assert!(browser.new_monitor.ignore_tls);
}

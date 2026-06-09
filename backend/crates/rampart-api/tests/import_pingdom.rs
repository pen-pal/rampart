//! Integration test for the Pingdom importer's parse + map pass.
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

use rampart_api::importers::{pingdom, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/pingdom-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = pingdom::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 entries total in the fixture; one is `transaction` which has no
    // Rampart equivalent and should land in `skipped`, not `mapped`.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");
    assert_eq!(plan.skipped[0].source_kind, "transaction");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Keyword), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = pingdom::parse_and_map(FIXTURE).expect("fixture parses");

    // The plain HTTP entry: full URL reconstructed from hostname +
    // encryption + path; default 443 dropped from the URL.
    let http = plan
        .mapped
        .iter()
        .find(|m| m.source_name == "Acme HTTPS health")
        .expect("http entry present");
    assert_eq!(http.mapped_kind, MonitorKind::Http);
    assert_eq!(
        http.new_monitor.url.as_deref(),
        Some("https://api.example.com/health")
    );
    assert_eq!(http.new_monitor.hostname, None);
    assert_eq!(http.new_monitor.http_method, "GET");
    // resolution 1 minute -> 60 seconds.
    assert_eq!(http.new_monitor.interval_seconds, 60);
    // verify_certificate=true -> ignore_tls=false.
    assert!(!http.new_monitor.ignore_tls);

    // The keyword entry: should_contain lands in config["keyword"] and
    // verify_certificate=false flips ignore_tls=true. resolution=5 -> 300s.
    let kw = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Keyword)
        .expect("keyword entry present");
    assert_eq!(kw.new_monitor.name, "Acme HTTPS w/ keyword");
    assert_eq!(
        kw.new_monitor.url.as_deref(),
        Some("https://app.example.com/")
    );
    assert_eq!(kw.new_monitor.interval_seconds, 300);
    assert!(kw.new_monitor.ignore_tls);
    assert_eq!(
        kw.new_monitor
            .config
            .get("keyword")
            .and_then(|v| v.as_str()),
        Some("Welcome to Acme")
    );

    // The TCP entry: hostname + port kept split; no URL emitted.
    let tcp = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Tcp)
        .expect("tcp entry present");
    assert_eq!(tcp.new_monitor.name, "Acme primary DB port");
    assert_eq!(
        tcp.new_monitor.hostname.as_deref(),
        Some("db.internal.example.com")
    );
    assert_eq!(tcp.new_monitor.port, Some(5432));
    assert_eq!(tcp.new_monitor.url, None);
    // resolution 5 minutes -> 300 seconds.
    assert_eq!(tcp.new_monitor.interval_seconds, 300);

    // The PING entry: hostname kept, no port, default timeout (16s).
    let ping = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Ping)
        .expect("ping entry present");
    assert_eq!(ping.new_monitor.hostname.as_deref(), Some("10.0.0.1"));
    assert_eq!(ping.new_monitor.port, None);
    assert_eq!(ping.new_monitor.timeout_seconds, 16);
}

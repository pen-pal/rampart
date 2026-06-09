//! Integration test for the Uptime.com importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the JSON fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running the importer.
//!
//! No DB needed — the dry-run path is the contract that matters.

use rampart_api::importers::{uptimecom, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/uptimecom-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = uptimecom::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 checks: HTTP, TCP, ICMP, SSL map; the TRANSACTION check has no
    // Rampart equivalent and is skipped.
    assert_eq!(plan.mapped.len(), 4, "mapped count");
    assert_eq!(plan.skipped.len(), 1, "skipped count");
    assert_eq!(plan.skipped[0].source_kind, "TRANSACTION");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
    assert_eq!(count(&plan, MonitorKind::Tls), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = uptimecom::parse_and_map(FIXTURE).expect("fixture parses");

    // HTTP: msp_address -> url; msp_interval (minutes) -> seconds.
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
    assert_eq!(site.new_monitor.interval_seconds, 60, "1 minute -> 60s");

    // TCP: msp_address -> hostname; msp_port -> port; 5 min -> 300s.
    let db = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Primary database")
        .expect("Primary database present");
    assert_eq!(db.mapped_kind, MonitorKind::Tcp);
    assert!(db.new_monitor.url.is_none());
    assert_eq!(db.new_monitor.hostname.as_deref(), Some("db.example.com"));
    assert_eq!(db.new_monitor.port, Some(5432));
    assert_eq!(db.new_monitor.interval_seconds, 300, "5 minutes -> 300s");

    // ICMP -> Ping, address as hostname.
    let router = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "Edge router")
        .expect("Edge router present");
    assert_eq!(router.mapped_kind, MonitorKind::Ping);
    assert_eq!(router.new_monitor.hostname.as_deref(), Some("10.0.0.1"));

    // SSL -> Tls, address as hostname, port carried; 10 min -> 600s.
    let cert = plan
        .mapped
        .iter()
        .find(|m| m.new_monitor.name == "TLS certificate")
        .expect("TLS certificate present");
    assert_eq!(cert.mapped_kind, MonitorKind::Tls);
    assert_eq!(cert.new_monitor.hostname.as_deref(), Some("example.com"));
    assert_eq!(cert.new_monitor.port, Some(443));
    assert_eq!(cert.new_monitor.interval_seconds, 600);
}

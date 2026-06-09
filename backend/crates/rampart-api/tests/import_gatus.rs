//! Integration test for the Gatus importer's parse + map pass.
//!
//! Lives at the `tests/` layer (not as a `#[cfg(test)]` mod in the
//! importer file) because it asserts on the *binary contract* the
//! `rampart-import` CLI relies on — the JSON fixture under
//! `tests/fixtures/` is the same shape a real operator would drop on
//! disk before running the importer.
//!
//! No DB needed — the dry-run path is the contract that matters.

use rampart_api::importers::{gatus, ImportPlan};
use rampart_core::MonitorKind;

const FIXTURE: &str = include_str!("fixtures/gatus-sample.json");

fn count(plan: &ImportPlan, kind: MonitorKind) -> usize {
    plan.mapped.iter().filter(|m| m.mapped_kind == kind).count()
}

#[test]
fn fixture_parses_and_maps_expected_kinds() {
    let plan = gatus::parse_and_map(FIXTURE).expect("fixture parses");

    // 5 endpoints, one per supported scheme; nothing skipped.
    assert_eq!(plan.mapped.len(), 5, "mapped count");
    assert!(plan.skipped.is_empty(), "skipped should be empty");

    assert_eq!(count(&plan, MonitorKind::Http), 1);
    assert_eq!(count(&plan, MonitorKind::Tcp), 1);
    assert_eq!(count(&plan, MonitorKind::Ping), 1);
    assert_eq!(count(&plan, MonitorKind::Dns), 1);
    assert_eq!(count(&plan, MonitorKind::Tls), 1);
}

#[test]
fn fixture_field_translation_is_correct() {
    let plan = gatus::parse_and_map(FIXTURE).expect("fixture parses");

    // HTTP endpoint: group prefixes the name, url kept verbatim.
    let web = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Http)
        .expect("http endpoint present");
    assert_eq!(web.new_monitor.name, "core/front-end", "group/name prefix");
    assert_eq!(
        web.new_monitor.url.as_deref(),
        Some("https://example.com/health")
    );
    assert!(web.new_monitor.hostname.is_none());
    // Status export carries no cadence; default 60s.
    assert_eq!(web.new_monitor.interval_seconds, 60);

    // TCP endpoint: scheme stripped, host + port split out.
    let db = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Tcp)
        .expect("tcp endpoint present");
    assert_eq!(db.new_monitor.name, "core/database");
    assert!(db.new_monitor.url.is_none());
    assert_eq!(db.new_monitor.hostname.as_deref(), Some("db.example.com"));
    assert_eq!(db.new_monitor.port, Some(5432));

    // ICMP endpoint: scheme stripped into hostname.
    let router = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Ping)
        .expect("icmp endpoint present");
    assert_eq!(router.new_monitor.name, "network/edge-router");
    assert_eq!(router.new_monitor.hostname.as_deref(), Some("10.0.0.1"));

    // DNS endpoint has no group -> bare name.
    let resolver = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Dns)
        .expect("dns endpoint present");
    assert_eq!(
        resolver.new_monitor.name, "resolver",
        "no group -> bare name"
    );
    assert_eq!(resolver.new_monitor.hostname.as_deref(), Some("1.1.1.1"));

    // starttls endpoint maps to Tls, host stripped of scheme.
    let mail = plan
        .mapped
        .iter()
        .find(|m| m.mapped_kind == MonitorKind::Tls)
        .expect("tls endpoint present");
    assert_eq!(mail.new_monitor.name, "mail/mail-relay");
    assert_eq!(
        mail.new_monitor.hostname.as_deref(),
        Some("smtp.example.com")
    );
}

//! Updown.io monitor export importer.
//!
//! Reads an Updown.io `GET /api/checks` JSON dump — operators capture
//! one with their own API key, save it to disk, hand the path to
//! `rampart-import updown`. We don't reach out to the Updown.io API
//! ourselves: importers are one-shot, offline tools by design (see
//! `docs/IMPORTERS.md` + `CONTRIBUTING.md` "Importers" bullet).
//!
//! ## Mapping
//!
//! Updown.io is **HTTP-only**: every check maps onto Rampart's `Http`
//! probe (or `Keyword` when the check has a `string_match` configured).
//! The top-level export shape is a **bare JSON array** — not the
//! `{"data":[…]}` envelope the other importers use — so we deserialise
//! straight into `Vec<Value>` and skip the `Export` struct dance.
//!
//! ## Field translation
//!
//! | Updown.io                | Rampart `NewMonitor`                                                                |
//! | ------------------------ | ----------------------------------------------------------------------------------- |
//! | `alias`                  | `name` (falls back to `url` when `alias` is empty)                                  |
//! | `url`                    | `url`                                                                               |
//! | `period` (seconds)       | `interval_seconds` (clamped to `10..=86400`; default `60`)                          |
//! | `http_verb`              | `http_method` (uppercased; default `GET`)                                           |
//! | `string_match`           | `config["keyword"]` (and flips `Http` → `Keyword` when present and non-empty)       |
//!
//! Updown.io ships `period` as a bare JSON number; we accept stringified
//! numbers defensively to match the leniency the sibling importers apply.

use serde_json::{Map, Value};
use tracing::warn;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;

pub use super::ImportError;
use super::{ImportPlan, MappedMonitor, SkippedMonitor};

/// Parse an Updown.io export and map every entry onto a `NewMonitor`.
/// Returns the mapped list + a list of skipped monitors (with reasons).
/// Pure function — no I/O, no DB; the integration test uses this
/// directly without standing up Postgres.
///
/// The Updown.io listing endpoint returns a bare JSON array at the top
/// level (not the `{"data":[…]}` envelope the other importers use), so
/// we deserialise straight into `Vec<Value>`.
pub fn parse_and_map(json: &str) -> Result<ImportPlan, ImportError> {
    let checks: Vec<Value> = serde_json::from_str(json)?;
    if checks.is_empty() {
        return Err(ImportError::NoMonitors);
    }

    let mut plan = ImportPlan::default();
    for raw in checks {
        match map_one(&raw) {
            Ok(m) => plan.mapped.push(m),
            Err(s) => {
                warn!(
                    source_name = %s.source_name,
                    source_kind = %s.source_kind,
                    reason = %s.reason,
                    "skip: unsupported updown check",
                );
                plan.skipped.push(s);
            }
        }
    }
    Ok(plan)
}

/// Map a single Updown.io check onto a Rampart `NewMonitor`. Returns
/// the mapped form or a `SkippedMonitor` describing why we couldn't
/// translate this one. The only thing we skip on is a missing URL —
/// every Updown check is HTTP-shaped and needs *something* to probe.
fn map_one(raw: &Value) -> Result<MappedMonitor, SkippedMonitor> {
    // Updown.io is HTTP-only; the source-kind label is constant. We
    // still thread it through `SkippedMonitor` for symmetry with the
    // other importers so a future API revision adding non-HTTP check
    // families can be surfaced without a structural change.
    let source_kind = "http".to_string();

    let url = string_field(raw, "url").filter(|s| !s.is_empty());
    let alias = string_field(raw, "alias").filter(|s| !s.is_empty());

    // Pick the best display name available: alias first (operator-
    // friendly), fall back to URL so we don't have to skip rows the
    // operator never explicitly labelled.
    let source_name = alias.clone().or_else(|| url.clone()).unwrap_or_default();

    if source_name.is_empty() {
        return Err(SkippedMonitor {
            source_name: "<unnamed>".into(),
            source_kind,
            reason: "missing alias and url".into(),
        });
    }

    let string_match = string_field(raw, "string_match").filter(|s| !s.is_empty());

    // Promote Http -> Keyword when the operator configured a body match
    // on the source check. Rampart's Keyword probe asserts substring
    // presence in the response body; Updown's `string_match` is the
    // same semantics.
    let mapped_kind = if string_match.is_some() {
        MonitorKind::Keyword
    } else {
        MonitorKind::Http
    };

    let http_method = string_field(raw, "http_verb")
        .map(|m| m.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "GET".into());

    let interval_seconds = numeric_field(raw, "period").unwrap_or(60).clamp(10, 86400);

    let mut config_obj = Map::new();
    if let Some(kw) = string_match {
        config_obj.insert("keyword".to_string(), Value::String(kw));
    }

    let new_monitor = NewMonitor {
        name: source_name.clone(),
        kind: mapped_kind,
        url,
        hostname: None,
        port: None,
        config: Value::Object(config_obj),
        interval_seconds,
        timeout_seconds: 16,
        max_retries: 0,
        retry_interval_sec: 60,
        resend_interval_sec: 0,
        upside_down: false,
        http_method,
        http_body: None,
        http_headers: None,
        accepted_statuses: vec![200, 201, 202, 203, 204, 205, 206, 207, 208, 226],
        follow_redirect: true,
        ignore_tls: false,
        proxy_id: None,
        group_id: None,
        slo_target_pct: None,
        slo_window_days: None,
        agent_id: None,
    };

    Ok(MappedMonitor {
        source_name,
        source_kind,
        mapped_kind,
        new_monitor,
    })
}

/// Pull a string out of the JSON object. Accepts both `"foo"` and `null`
/// (returns `None` for null), and tolerates a missing key.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Pull a numeric-ish field out of the JSON. Updown.io ships numbers
/// as bare JSON numbers per the documented schema, but accept
/// stringified numbers defensively.
fn numeric_field(v: &Value, key: &str) -> Option<i32> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().map(|x| x as i32),
        Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_maps_to_http() {
        let json = r#"[
            {"token":"a","url":"https://a.example.com","alias":"A","period":60,"http_verb":"GET"},
            {"token":"b","url":"https://b.example.com","alias":"B","period":120,"http_verb":"HEAD"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped.len(), 2);
        for m in &plan.mapped {
            assert_eq!(m.mapped_kind, MonitorKind::Http);
        }
    }

    #[test]
    fn string_match_promotes_to_keyword() {
        let json = r#"[
            {"token":"a","url":"https://x","alias":"Body check","period":60,"http_verb":"GET","string_match":"Welcome"}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].mapped_kind, MonitorKind::Keyword);
        assert_eq!(plan.mapped[0].new_monitor.config["keyword"], "Welcome");
    }

    #[test]
    fn alias_used_as_name_when_present() {
        let json = r#"[
            {"token":"a","url":"https://x","alias":"My alias","period":60}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.name, "My alias");
    }

    #[test]
    fn url_used_as_name_when_alias_missing_or_empty() {
        let json = r#"[
            {"token":"a","url":"https://no-alias.example.com","period":60},
            {"token":"b","url":"https://empty-alias.example.com","alias":"","period":60}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(
            plan.mapped[0].new_monitor.name,
            "https://no-alias.example.com"
        );
        assert_eq!(
            plan.mapped[1].new_monitor.name,
            "https://empty-alias.example.com"
        );
    }

    #[test]
    fn http_verb_is_uppercased_and_defaults_to_get() {
        let json = r#"[
            {"token":"a","url":"https://x","alias":"a","period":60,"http_verb":"post"},
            {"token":"b","url":"https://y","alias":"b","period":60}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.http_method, "POST");
        assert_eq!(plan.mapped[1].new_monitor.http_method, "GET");
    }

    #[test]
    fn period_translates_to_interval_and_is_clamped() {
        let json = r#"[
            {"token":"a","url":"https://x","alias":"fast","period":5},
            {"token":"b","url":"https://y","alias":"slow","period":99999},
            {"token":"c","url":"https://z","alias":"normal","period":300}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert_eq!(plan.mapped[0].new_monitor.interval_seconds, 10);
        assert_eq!(plan.mapped[1].new_monitor.interval_seconds, 86400);
        assert_eq!(plan.mapped[2].new_monitor.interval_seconds, 300);
    }

    #[test]
    fn skips_when_neither_alias_nor_url_present() {
        let json = r#"[
            {"token":"a","period":60}
        ]"#;
        let plan = parse_and_map(json).unwrap();
        assert!(plan.mapped.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn empty_array_errors() {
        let json = r#"[]"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Parse(_) | ImportError::NoMonitors
        ));
    }

    #[test]
    fn non_array_top_level_errors() {
        let json = r#"{"checks":[]}"#;
        let err = parse_and_map(json).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }
}

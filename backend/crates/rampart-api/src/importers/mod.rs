//! Importer modules — one-shot adapters that turn third-party monitor
//! exports into `NewMonitor` payloads we can hand to
//! [`rampart_db::monitors::create`].
//!
//! Importers live in the API crate (not their own) because they share
//! the DB pool + sqlx + rampart-db dep tree the `rampart-import` bin
//! needs anyway. Each module exposes a `parse_and_map(json) ->
//! Vec<MappedMonitor>` so the parser + mapping is unit-testable without
//! standing up a DB.

pub mod betterstack;
pub mod cachet;
pub mod checkly;
pub mod cronitor;
pub mod csv_import;
pub mod datadog;
pub mod freshping;
pub mod gatus;
pub mod healthchecks;
pub mod hetrixtools;
pub mod pingdom;
pub mod pingometer;
pub mod rapidspike;
pub mod site24x7;
pub mod statuscake;
pub mod statusgator;
pub mod updown;
pub mod uptimecom;
pub mod uptimerobot;

use rampart_core::monitor::NewMonitor;
use rampart_core::MonitorKind;
use thiserror::Error;

/// Shared error type for every importer's `parse_and_map` entry point.
/// Lives here (not in any single importer module) so a second / third /
/// Nth format can return the same variants without restating them. The
/// CLI in `src/bin/import.rs` only ever sees this enum, so the dispatch
/// arm doesn't need to know which importer produced the error.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("failed to parse export: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("export had no monitors array at the top level — pointed at the wrong file?")]
    NoMonitors,

    #[error("failed to parse CSV: {0}")]
    Csv(String),
}

/// One Site24x7 (or other source) monitor that successfully mapped
/// onto a Rampart `NewMonitor`. The source-side `display_name` is
/// preserved as `source_name` for log lines + `--skip-existing` lookup.
#[derive(Debug, Clone)]
pub struct MappedMonitor {
    pub source_name: String,
    pub source_kind: String,
    pub mapped_kind: MonitorKind,
    pub new_monitor: NewMonitor,
}

/// One monitor we couldn't map. Reported to the operator at the end of
/// the run so they can hand-port the unusual ones.
#[derive(Debug, Clone)]
pub struct SkippedMonitor {
    pub source_name: String,
    pub source_kind: String,
    pub reason: String,
}

/// Outcome of a parse + map pass. The binary drives this through
/// `parse_and_map` and either prints a summary (`--dry-run`) or feeds
/// the `mapped` list into `monitors::create`.
#[derive(Debug, Default)]
pub struct ImportPlan {
    pub mapped: Vec<MappedMonitor>,
    pub skipped: Vec<SkippedMonitor>,
}

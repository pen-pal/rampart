//! MySQL/MariaDB backend — multi-DB P2 (relational subset for MySQL shops).
//!
//! Behind the `mysql` cargo feature so the default Postgres build + its `.sqlx`
//! cache are untouched. See `docs/design/MULTI_DB.md` (P2 plan). This is the P0
//! spike: it proves the toolchain (driver + `#[sqlx::test]` + the upsert
//! dialect) on `settings`; later slices add real domains the way the SQLite
//! backend did, then a full `impl Store for MysqlStore`.
//!
//! ## Why runtime-checked queries (not `query!`)
//!
//! Same reason as the SQLite layer: one crate can't hold three sets of `query!`
//! macros (each validates against a single `DATABASE_URL`), so the MySQL layer
//! uses runtime-checked `sqlx::query`/`query_as` — builds under `SQLX_OFFLINE`
//! alongside the PG cache; correctness is covered by `#[sqlx::test]` against the
//! CI MySQL service.
//!
//! ## Dialect conventions (see `migrations-mysql/`)
//!
//! - uuids → `CHAR(36)` (hyphenated); timestamps → `BIGINT` unix-seconds;
//!   `?` placeholders; JSON → `LONGTEXT` (or native `JSON` + `JSON_EXTRACT` for
//!   value-querying domains); upserts → `INSERT … ON DUPLICATE KEY UPDATE`;
//!   no `RETURNING` (app-side UUID PK + INSERT-then-SELECT); no array binds
//!   (bound `IN (?,…)` lists, as in the SQLite layer).

pub mod agents;
pub mod api_keys;
pub mod audit;
pub mod delivery_log;
pub mod deploy_markers;
pub mod detection;
pub mod digest_buffer;
pub mod escalations;
pub mod heartbeats;
pub mod incident_templates;
pub mod ingest_keys;
pub mod logs;
pub mod maintenance;
pub mod metric_rules;
pub mod metric_samples;
pub mod monitor_groups;
pub mod monitor_presets;
pub mod monitor_templates;
pub mod monitors;
pub mod notifications;
pub mod oidc_state;
pub mod on_call;
pub mod orgs;
pub mod profiles;
pub mod proxies;
pub mod recovery_codes;
pub mod routing;
pub mod scheduled_reports;
pub mod sessions;
pub mod settings;
pub mod silences;
pub mod slos;
pub mod source_maps;
pub mod store;
pub mod tags;
pub mod telemetry_rules;
pub mod templates;
pub mod traces;
pub mod users;
pub mod webpush;

use rampart_core::ids::{OrgId, UserId};
use rampart_core::Role;
use time::OffsetDateTime;
use uuid::Uuid;

// Dialect-neutral decode/encode helpers. Identical to the SQLite layer's
// (uuid→CHAR(36) hyphenated, ts→BIGINT unix-seconds, role enum↔TEXT) — copied
// rather than shared because the SQLite module is feature-gated off in a
// MySQL-only build.
// ponytail: tiny duplication across two backend modules; fold into a shared
// `crate::dialect` helper module if a third relational backend lands.

/// Decode a BIGINT unix-seconds column to [`OffsetDateTime`].
pub(crate) fn ts(secs: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(secs).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// Parse a CHAR(36) uuid column into an `OrgId` (nil on corrupt).
pub(crate) fn oid(s: &str) -> OrgId {
    OrgId::from_uuid(Uuid::parse_str(s).unwrap_or(Uuid::nil()))
}

/// Parse a CHAR(36) uuid column into a `UserId`.
pub(crate) fn uid(s: &str) -> UserId {
    UserId::from_uuid(Uuid::parse_str(s).unwrap_or(Uuid::nil()))
}

/// Parse a CHAR(36) uuid column into a raw `Uuid` (session ids, active_org_id).
pub(crate) fn raw_uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or(Uuid::nil())
}

/// The Default org's id (seeded by `migrations-mysql/0002_identity.sql`), as a
/// string for binding. Mirrors `rampart_core::org::DEFAULT_ORG_ID`.
pub(crate) fn default_org_id_str() -> String {
    OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID)
        .0
        .to_string()
}

/// `Role` → the TEXT form stored in MySQL (the PG `user_role` labels).
pub(crate) fn role_str(r: Role) -> &'static str {
    match r {
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Readonly => "readonly",
    }
}

/// TEXT role → `Role` (unknown/corrupt → `Editor`, least-privilege-ish default).
pub(crate) fn role_from(s: &str) -> Role {
    match s {
        "admin" => Role::Admin,
        "readonly" => Role::Readonly,
        _ => Role::Editor,
    }
}

/// Parse a CHAR(36) uuid column into a `MonitorId`.
pub(crate) fn mid(s: &str) -> rampart_core::ids::MonitorId {
    rampart_core::ids::MonitorId::from_uuid(raw_uuid(s))
}

/// `MonitorKind`/`MonitorStatus` ↔ their TEXT form via the serde `rename_all`
/// labels (matches the PG enum labels), avoiding a 40-arm match.
pub(crate) fn kind_str(k: rampart_core::monitor::MonitorKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "http".into())
}
pub(crate) fn kind_from(s: &str) -> rampart_core::monitor::MonitorKind {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .unwrap_or(rampart_core::monitor::MonitorKind::Http)
}
pub(crate) fn mstatus_str(s: rampart_core::monitor::MonitorStatus) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "pending".into())
}
pub(crate) fn mstatus_from(s: &str) -> rampart_core::monitor::MonitorStatus {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .unwrap_or(rampart_core::monitor::MonitorStatus::Pending)
}

/// `IN (?,?,…)` placeholder list of length `n` (n >= 1). Count from the slice;
/// values always bound, never interpolated. Shared by the batch hydrators.
pub(crate) fn in_placeholders(n: usize) -> String {
    let mut s = String::with_capacity(n * 2);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s
}

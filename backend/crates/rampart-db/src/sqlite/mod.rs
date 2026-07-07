//! SQLite backend — multi-DB P1 (single-binary / homelab tier).
//!
//! Behind the `sqlite` cargo feature so the default Postgres build + its `.sqlx`
//! cache are untouched. See `docs/design/MULTI_DB.md` (P1 plan).
//!
//! ## Status: building out `SqliteStore` domain-by-domain
//!
//! P1-0 proved the toolchain on `settings`; subsequent slices add real domains
//! ([`orgs`] is the identity/tenancy core). Each domain mirrors the Postgres
//! free-fn surface (so the eventual `impl Store for SqliteStore` just delegates,
//! like `PgStore`) but is hand-written against SQLite. A full `impl Store` waits
//! until enough domains exist — the super-trait needs every method.
//!
//! ## Why runtime-checked queries (not `query!`)
//!
//! One crate can't hold both Postgres and SQLite `query!` macros: compile-time
//! validation hits a single `DATABASE_URL` (PG queries fail a describe against
//! SQLite and vice-versa), and the offline `.sqlx` cache can't hold both because
//! each `cargo sqlx prepare` prunes the entries it didn't compile. Gating the
//! entire PG query layer behind `cfg(feature = "postgres")` is a large later
//! slice; until then the SQLite layer uses runtime-checked `sqlx::query`/
//! `query_as` (compiles under `SQLX_OFFLINE` alongside the PG cache), with
//! correctness covered by the `#[sqlx::test]` suite.
//!
//! ## Dialect conventions (see `migrations-sqlite/`)
//!
//! - uuids stored as TEXT (hyphenated); timestamps as INTEGER unix-seconds
//!   (`unixepoch()`) for a clean `i64` ↔ [`OffsetDateTime`] round-trip;
//! - `?` placeholders; JSON as TEXT; `ON CONFLICT … DO UPDATE SET col =
//!   excluded.col`; the `user_role` enum as a CHECK'd TEXT column.

pub mod agents;
pub mod audit;
pub mod delivery_log;
pub mod detection;
pub mod digest_buffer;
pub mod escalations;
pub mod heartbeats;
pub mod ingest_keys;
pub mod logs;
pub mod maintenance;
pub mod metric_rules;
pub mod metric_samples;
pub mod monitor_groups;
pub mod monitors;
pub mod notifications;
pub mod orgs;
pub mod profiles;
pub mod proxies;
pub mod routing;
pub mod rum;
pub mod scheduled_reports;
pub mod sessions;
pub mod settings;
pub mod silences;
pub mod slos;
pub mod store;
pub mod tags;
pub mod telemetry_rules;
pub mod templates;
pub mod traces;
pub mod users;

/// `IN (?,?,…)` placeholder list of length `n` (n >= 1). The count comes from
/// the slice; values are always `.bind()`-ed, never interpolated — so the
/// `AssertSqlSafe`-wrapped SQL it feeds stays injection-safe. Shared by the
/// batch helpers in `tags`/`heartbeats`.
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

/// Conservative per-statement bind ceiling for multi-row `INSERT`s. SQLite's
/// `SQLITE_MAX_VARIABLE_NUMBER` defaults to 999 on the libsqlite3 amalgam sqlx
/// bundles; staying under it keeps batched inserts portable regardless of build
/// flags. [`insert_chunk`] turns this into a row count for a given column width.
pub(crate) const SQLITE_MAX_BIND: usize = 900;

/// Rows per batched `INSERT` given `ncols` bind params per row (>= 1).
pub(crate) const fn insert_chunk(ncols: usize) -> usize {
    let c = SQLITE_MAX_BIND / ncols;
    if c == 0 {
        1
    } else {
        c
    }
}

/// Chunked age-based delete (SQLite): repeatedly delete up to
/// [`crate::prune::PRUNE_BATCH`] rows where `ts_col` < `cutoff` (a unix epoch
/// second) until fewer than a full batch remain. Bounds each DELETE's
/// write-lock + WAL footprint so a large retention backlog is cleared in many
/// short statements. SQLite only supports `DELETE ... LIMIT` when the amalgam is
/// built with `SQLITE_ENABLE_UPDATE_DELETE_LIMIT` (not guaranteed), so this
/// limits via a `rowid IN (SELECT ... LIMIT ?)` subquery, which is portable.
/// `table`/`ts_col` are crate-internal literals (no injection surface), hence
/// `AssertSqlSafe`. Returns total rows deleted.
pub(crate) async fn chunked_delete_older(
    pool: &sqlx::SqlitePool,
    table: &str,
    ts_col: &str,
    cutoff: i64,
) -> crate::DbResult<u64> {
    let sql = format!(
        "DELETE FROM {table} WHERE rowid IN \
         (SELECT rowid FROM {table} WHERE {ts_col} < ? LIMIT ?)"
    );
    let mut total = 0u64;
    loop {
        let n = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .bind(cutoff)
            .bind(crate::prune::PRUNE_BATCH)
            .execute(pool)
            .await?
            .rows_affected();
        total += n;
        if n < crate::prune::PRUNE_BATCH as u64 {
            break;
        }
    }
    Ok(total)
}

/// The Default org's id (seeded by `migrations-sqlite/0002_identity.sql`), as a
/// string for binding. Mirrors `rampart_core::org::DEFAULT_ORG_ID`.
pub(crate) fn default_org_id_str() -> String {
    OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID)
        .0
        .to_string()
}

use rampart_core::ids::{OrgId, UserId};
use rampart_core::Role;
use time::OffsetDateTime;
use uuid::Uuid;

/// `Role` → the TEXT form stored in SQLite (the Postgres `user_role` labels).
pub(crate) fn role_str(r: Role) -> &'static str {
    match r {
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Readonly => "readonly",
    }
}

/// TEXT role → `Role` (unknown/corrupt → `Editor`, the safe least-privilege-ish
/// default, matching how serde would reject and callers fall back).
pub(crate) fn role_from(s: &str) -> Role {
    match s {
        "admin" => Role::Admin,
        "readonly" => Role::Readonly,
        _ => Role::Editor,
    }
}

/// Decode an INTEGER unix-seconds column to [`OffsetDateTime`].
pub(crate) fn ts(secs: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(secs).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// Parse a TEXT uuid column into an `OrgId` (nil on corrupt — our own ids always
/// round-trip, so this only guards against external tampering).
pub(crate) fn oid(s: &str) -> OrgId {
    OrgId::from_uuid(Uuid::parse_str(s).unwrap_or(Uuid::nil()))
}

/// Parse a TEXT uuid column into a `UserId`.
pub(crate) fn uid(s: &str) -> UserId {
    UserId::from_uuid(Uuid::parse_str(s).unwrap_or(Uuid::nil()))
}

/// Parse a TEXT uuid column into a raw `Uuid` (session ids, active_org_id).
pub(crate) fn raw_uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or(Uuid::nil())
}

/// Parse a TEXT uuid column into a `MonitorId`.
pub(crate) fn mid(s: &str) -> rampart_core::ids::MonitorId {
    rampart_core::ids::MonitorId::from_uuid(raw_uuid(s))
}

/// `MonitorKind`/`MonitorStatus` ↔ their TEXT form. The enums carry serde
/// `rename_all` (snake_case / lowercase) matching the PG enum labels, so we
/// round-trip through serde instead of hand-writing a 40-arm match.
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

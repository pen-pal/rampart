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

pub mod orgs;
pub mod settings;

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

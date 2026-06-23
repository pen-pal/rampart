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

pub mod orgs;
pub mod sessions;
pub mod settings;
pub mod users;

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

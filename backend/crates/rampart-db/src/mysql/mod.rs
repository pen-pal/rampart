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

pub mod settings;

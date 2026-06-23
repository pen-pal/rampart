# Changelog

All notable changes to Rampart are recorded here.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/)
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html):

- **MAJOR** — incompatible API changes, schema migrations that require a manual step, or removal of a probe / channel kind.
- **MINOR** — new probe kinds, new notification channels, new endpoints, new dashboard views — anything additive.
- **PATCH** — bug fixes, security fixes, dependency bumps, documentation, packaging.

The version baked into the binary (and surfaced on `/healthz` + the dashboard header) is the single source of truth — it inherits from `[workspace.package].version` in `backend/Cargo.toml`. Frontend `package.json` tracks the same value.

For the procedure to cut a release see [`docs/RELEASING.md`](docs/RELEASING.md).

---

## [Unreleased]

### Helm chart (v0.4.0)
- **Zero-downtime rolling updates + node-disk-fill guard.** The Deployment now
  sets `terminationGracePeriodSeconds: 30` and an opt-out `preStop` sleep
  (`preStopSleepSeconds: 5`) so the ingress/kube-proxy deregisters the endpoint
  before the app drains — rolling updates drop zero in-flight requests. The
  `/tmp` scratch `emptyDir` (required by `readOnlyRootFilesystem`) gets a
  `sizeLimit` (256Mi) and the container gets `ephemeral-storage` requests/limits,
  so runaway scratch can't fill the node and evict neighbours. (six-persona audit
  ranks 2 + 3.)

---

## [0.156.70] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `routing` junction helpers.** The dispatch-path
  `resolve_channels_for_monitor` (recursive folder-walk union) was already
  wired; this adds the 12 remaining `StoreRouting` methods — read helpers
  (group_tag_ids / channel_tag_ids / group_channel_ids / monitor_exclude_ids)
  and junction mutators (tag_group / untag_group / tag_channel / untag_channel /
  attach_group_channel / detach_group_channel / exclude_channel /
  unexclude_channel) over `group_tags` / `notification_tags` /
  `group_notifications` / `monitor_notification_excludes` (no migration — the
  junction tables exist). `ON CONFLICT DO NOTHING`→`INSERT IGNORE`. +1
  `#[sqlx::test]` (group↔tag, channel↔tag, group↔channel, monitor-exclude
  round-trips incl. idempotent insert) green on MariaDB. PG + SQLite untouched.

## [0.156.69] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `monitor_groups` folder tree + dependency graph.**
  The dispatch-path `any_parent_down` suppression read was already wired; this
  adds in_org / list / create / update / would_form_group_cycle / delete +
  parents_of / children_of / attach_dependency / detach_dependency /
  would_form_cycle to `mysql/monitor_groups.rs` and un-stubs the 11 remaining
  `StoreMonitorGroups` methods (no migration — `monitor_groups` +
  `monitor_dependencies` tables exist). The folder-tree + dependency-DAG cycle
  guards walk the graph in Rust (reused shape); `update` gates via `in_org`
  first (MySQL counts CHANGED rows, so a no-op COALESCE can't carry the gate);
  tri-state reparent is a separate UPDATE; `ON CONFLICT DO NOTHING`→`INSERT
  IGNORE`. +1 `#[sqlx::test]` (folder create/list/reparent + cycle guard,
  dependency attach/detach + self/cycle rejection + pending-parent suppression,
  cross-org gate, delete) green on MariaDB. PG + SQLite untouched.

## [0.156.68] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `silences` CRUD** (alert suppression). The
  dispatch-path `is_silenced` chokepoint was already wired; this adds
  create/list_active/delete to `mysql/silences.rs` and un-stubs the 3
  `StoreSilences` management methods (no migration — the table exists). App-side
  UUID (no RETURNING), `now()`→bound cutoff, `list_active` LEFT JOINs monitors
  for the name, org-scoped delete returns a bool. +1 `#[sqlx::test]`
  (global-vs-scoped silencing, active listing, cross-org delete no-op, expired
  silence ignored) green on MariaDB. PG + SQLite untouched.

## [0.156.67] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `templates` CRUD** (notification subject/body
  templates). The dispatch-path `get_render_strings` was already wired; this
  adds list/get/create/update/delete to `mysql/templates.rs` and un-stubs the 5
  `StoreTemplates` CRUD methods. `migrations-mysql/0033_template_uniqueness.sql`
  adds the per-org `(org_id, name)` UNIQUE index (matching PG 0113) so a
  duplicate name surfaces a friendly `Conflict`. `channel_kinds` TEXT[]→LONGTEXT
  (serde JSON array), `is_default`→TINYINT, no RETURNING → re-select, `update`
  is get-cur-then-set-all. +1 `#[sqlx::test]` (create, per-org name conflict,
  partial update incl. subject clear + default flip, render-strings read,
  cross-org isolation, delete) green on MariaDB. PG + SQLite untouched.

## [0.156.66] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `error_tracking` domain** (Sentry-lite projects /
  issues / events; the biggest tail domain). `migrations-mysql/0032_error_tracking.sql`
  (error_projects + error_issues + error_events, native JSON context/stacktrace,
  the `(project_id, fingerprint)` grouping UNIQUE) + `mysql/error_tracking.rs`
  un-stubs all 23 `StoreErrorTracking` methods. Key dialect work: the ingest
  upsert `ON CONFLICT (project_id,fingerprint) DO NOTHING RETURNING id` →
  app-side UUID + `INSERT IGNORE` (rows_affected==1 ⇒ new issue claimed, else
  read-status-and-bump with resolved→unresolved regression); JSONB→native JSON
  with `JSON_UNQUOTE(JSON_EXTRACT(context,'$.user.…'))` for affected-users /
  stats (portable across MySQL + MariaDB, not the `->>'` operator); `release`
  backticked (reserved word); `date_bin` histogram → integer `DIV` bucketing;
  the retention prune → a multi-table `DELETE e FROM error_events e JOIN
  error_projects p …`; no FK cascade → `delete` drops events + issues in a tx.
  +1 `#[sqlx::test]` (project CRUD + slug, find-or-create idempotency,
  fingerprint grouping + times_seen, resolve→regression, affected-users /
  release / env stats, trace cross-link, histogram, cross-org gate, delete
  cascade, prune) green on MariaDB. PG + SQLite untouched.

## [0.156.65] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `ingest_tokens` domain** (page-scoped inbound webhook
  credentials). `migrations-mysql/0031_ingest_tokens.sql` + `mysql/ingest_tokens.rs`
  un-stubs the 7 `StoreIngestTokens` methods (create / create_with_token /
  set_mapping / list_for_page / find_by_token / delete / touch_last_used).
  Dual-write token + token_hash; `find_by_token` is hash-primary with a
  plaintext fallback (reuses `api_keys::sha256_hex` so hashes match PG); mapping
  jsonb→LONGTEXT; org tenanting flows through the owning status page via an
  `IN (SELECT … org_id)` gate. `set_mapping` re-selects through the gate so a
  same-value no-op UPDATE doesn't false-404 (MySQL counts CHANGED rows). +1
  `#[sqlx::test]` (create + hash-primary lookup, mapping set + idempotent re-set,
  deterministic create_with_token + duplicate conflict, last-used bump, cross-org
  gate, delete) green on MariaDB. PG + SQLite untouched.

## [0.156.64] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `subscribers` domain** (status-page email
  subscribers). `migrations-mysql/0030_subscribers.sql` (status_page_subscribers
  table) + `mysql/subscribers.rs` un-stubs `StoreSubscribers` (11 methods:
  subscribe_email / list_for_page / confirmed_emails_for_page / delete /
  unsubscribe_by_token / email_for_token / subscriptions_for_email /
  unsubscribe_all_for_email / unsubscribe_email_from_page / page_for /
  token_for) and adds `maintenance::confirmed_subscriber_emails_for_monitors`
  (un-stubbing `StoreMaintenance::confirmed_subscriber_emails_for_monitors`,
  which joins subscribers↔page-monitors). Single-opt-in (rows land confirmed),
  page-scoped tenanting; subscribe is idempotent SELECT-then-INSERT;
  `lower(x)=lower(?)` ports verbatim. +1 `#[sqlx::test]` (idempotent
  case-insensitive subscribe, multi-page manage view, token round-trips,
  per-page + all unsubscribe, NotFound paths, monitor→subscriber reach) green
  on MariaDB. PG + SQLite untouched.

## [0.156.63] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `status_pages` domain** (branded public status pages
  + component sections + the public projection). `mysql/status_pages.rs`
  un-stubs `StoreStatusPages` (16 methods: list / list_all / get / get_by_slug /
  get_unscoped / find_by_custom_domain / create / update / delete / public_view
  / verify_password / list_sections / create_section / update_section /
  delete_section / assign_monitor_section) and adds
  `maintenance::public_for_status_page` (un-stubbing
  `public_maintenance_for_status_page`). `password_hash IS NOT NULL` → the
  derived `private` boolean (Argon2 hash never leaves the db on a read path);
  the create/update password paths hash with Argon2id, same as PG.
  `public_view` keeps the set-based batch rollup (five queries regardless of
  monitor count) over `monitors`/`heartbeats` MySQL batch helpers + incidents +
  maintenance. No FK cascade on the MySQL tier → `delete` cascades child rows
  (incident_updates → incidents → page monitors → sections → page) in a tx by
  hand; `delete_section` detaches its monitors to ungrouped first. Duplicate-key
  → friendly `Conflict`, slug-vs-custom_domain disambiguated by the error
  message. +1 `#[sqlx::test]` (CRUD, slug conflict, password set/verify/clear →
  private toggle, sections append/rename/delete, monitor attach + section
  assignment + public_view bucketing, delete cascade, cross-org isolation) green
  on MariaDB. PG + SQLite untouched.

## [0.156.62] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `incidents` domain** (status-page announcements) +
  the status-page subsystem schema. `migrations-mysql/0029_status_pages.sql`
  provisions the whole subsystem (status_pages, status_page_sections,
  status_page_monitors, incidents, incident_updates) in one migration because
  the domains are mutually coupled — `incidents.recent` JOINs `status_pages`,
  `status_pages.public_view` reads incidents; the status_pages domain module
  follows in the next slice. `mysql/incidents.rs` un-stubs `StoreIncidents` (12
  methods: create / find_active_by_dedup_key / list_active / recent /
  list_resolved_history / resolve / list_all / delete / update / get /
  list_updates / post_update). bool→TINYINT(1), `incident_style` enum→VARCHAR
  (serde round-trip), no RETURNING → INSERT-then-re-select, `COALESCE` partial
  update ports verbatim. `resolve` swaps the rows_affected-NotFound gate for an
  existence probe (MySQL counts CHANGED rows, so re-resolve is a 0-change no-op
  that must stay idempotent). The PG partial UNIQUE `(status_page_id, dedup_key)
  WHERE active` has no MySQL equivalent — documented as a non-correctness
  integrity guard (the dedup lookup already LIMIT-1s). +1 `#[sqlx::test]`
  (CRUD, dedup hit/miss, running updates, partial edit, idempotent re-resolve,
  ghost-id NotFound, cross-org recent isolation) green on MariaDB. PG + SQLite
  untouched.

## [0.156.61] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `rum` domain** (Real User Monitoring, Tier 4).
  `migrations-mysql/0028_rum.sql` + 1 module, un-stubbing `StoreRum` (insert_event
  / page_samples / recent_traced / summary / pages / browser_breakdown /
  user_breakdown / apps / prune, all org-scoped). The key dialect delta: PG's p75
  reads use `percentile_cont(0.75) WITHIN GROUP` as an aggregate, which MySQL
  lacks (MariaDB exposes it window-only) — so the breakdown reads fetch the
  windowed rows and aggregate (group + count + linear-interpolation p75 +
  last_seen) app-side, bounded by short RUM retention × the read window. `ILIKE`
  UA buckets → app-side lowercase `contains`; `make_interval` → Rust cutoff;
  `DOUBLE PRECISION` → `DOUBLE`. +1 `#[sqlx::test]` (p75 correctness, page/
  browser/user rollups, traced feed, apps dropdown, cross-org isolation, prune)
  green on MariaDB. PG + SQLite untouched.

## [0.156.60] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `profiles` domain** (continuous-profiling tier).
  `migrations-mysql/0027_profiles.sql` + 1 module, un-stubbing `StoreProfiles`
  (insert / list / folded_in_window / fetch_folded / services / profile_types /
  prune, all org-scoped). PG `BIGSERIAL` → `BIGINT AUTO_INCREMENT` (no RETURNING
  → `LAST_INSERT_ID()`); `BYTEA folded` → `LONGBLOB`; `JSONB labels` → `LONGTEXT`
  (serde round-trip); `now() - make_interval(hours=>?)` → Rust-computed cutoff;
  optional service/type filters via the `(? IS NULL OR col = ?)` bind-twice form.
  +1 `#[sqlx::test]` (insert/list scoped+miss, window-blob merge set, pickers,
  single-fetch bytes, cross-org isolation, age-based prune) green on MariaDB.
  PG + SQLite untouched.

## [0.156.59] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `incident_templates` domain** (management-API tail,
  slice 6; canned incident-update bodies). `migrations-mysql/0026_incident_templates.sql`
  + 1 module, un-stubbing `StoreIncidentTemplates` (list/get/create/update/delete,
  all org-scoped). The PG `incident_style` enum → `VARCHAR(16)` via serde
  round-trip (like `monitors.kind`); no RETURNING → INSERT/UPDATE-then-re-select;
  `update` is read-modify-write (get-first confirms existence + `NotFound`, mirroring
  PG). +1 `#[sqlx::test]` (CRUD incl. style default, partial-update field retention,
  cross-org isolation) green on MariaDB. PG + SQLite untouched.

## [0.156.58] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `api_keys` + `ingest_keys` domains** (management-API
  tail, slice 5; the credential pair). `migrations-mysql/0025_api_keys_ingest_keys.sql`
  + 2 modules, un-stubbing `StoreApiKeys` (list/create/delete/lookup/
  touch_last_used — bearer keys, SHA-256-hashed, lookup on the UNIQUE key_hash;
  the legacy `scopes` array dropped, `scope` authoritative) and `StoreIngestKeys`
  (create/find_by_token/touch/list/delete — per-org ingest credentials, dual-write
  token + token_hash with hash-primary lookup + plaintext fallback). Both reuse
  `crate::api_keys::sha256_hex` so hashes match the PG store. TEXT[]→LONGTEXT(JSON),
  ts→BIGINT, no RETURNING → re-select. +2 `#[sqlx::test]` (incl. bearer lookup +
  cross-org isolation + origins round-trip) green on MariaDB. PG + SQLite untouched.

## [0.156.57] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `on_call` + `webpush` domains** (management-API tail,
  slice 4; batched). `migrations-mysql/0024_oncall_webpush.sql` + 2 modules,
  un-stubbing `StoreOnCall` (8 fns — on-call schedules + the "who's on call now"
  resolver; rotation math reuses pure `rampart_core::on_call`) and `StoreWebpush`
  (6 fns — web-push subscriptions keyed on a UNIQUE endpoint + the shared VAPID
  keypair stored in `settings`). **This closes the last two feature-conditional
  notifier-dispatch gaps** — a `mysql://` boot with on-call-targeted escalation
  steps or web-push channels no longer hits an `unimplemented!()`. Ported from PG:
  JSONB→LONGTEXT; anchor/ts→BIGINT; `ON CONFLICT(endpoint)` → `ON DUPLICATE KEY`;
  VAPID via `mysql::settings`. +2 `#[sqlx::test]` green on MariaDB. PG + SQLite
  untouched.

## [0.156.56] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `monitor_presets` + `monitor_templates` + `oidc_state`**
  (management-API tail, slice 3; 3 standalone domains batched into one migration +
  release). `migrations-mysql/0023_presets_templates_oidc.sql` + 3 modules,
  un-stubbing `StoreMonitorPresets`, `StoreMonitorTemplates`, `StoreOidcState`.
  Ported from PG: JSONB→LONGTEXT; `kind` CHECK ported; `state` TEXT PK →
  VARCHAR(64). **`oidc_state::consume` emulates PG's replay-safe `DELETE …
  RETURNING` with a tx `SELECT … FOR UPDATE` (row lock) → capture → `DELETE` →
  commit** — same one-time-use guarantee (a racing replay blocks on the lock,
  then finds nothing). +3 `#[sqlx::test]` (incl. the oidc one-time-use + expiry
  + prune path) green on MariaDB. PG + SQLite untouched.

## [0.156.55] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `recovery_codes` + `source_maps` domains** (management-
  API tail, slice 2; batched into one migration + one release to cut rebuild
  churn). `migrations-mysql/0022_recovery_codes_source_maps.sql` + both modules,
  un-stubbing `StoreRecoveryCodes` (issue/consume/delete/remaining — hashed
  one-shot TOTP codes) and `StoreSourceMaps` (upsert/get/list/delete — error-tier
  symbolication maps). Ported from PG: `release` is a MySQL reserved word →
  backticked; BIGSERIAL→`BIGINT AUTO_INCREMENT`; `ON CONFLICT … RETURNING id` →
  `ON DUPLICATE KEY UPDATE` + re-select id by the unique key. +2 `#[sqlx::test]`
  green on MariaDB. PG + SQLite untouched.

## [0.156.54] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `deploy_markers` domain** (management-API tail, slice
  1/N): deploy-timeline annotations (create / list_window / delete) +
  `migrations-mysql/0021_deploy_markers.sql`, un-stubbing `StoreDeployMarkers` on
  `MysqlStore`. First of the management-API domains — these have **no SQLite
  reference** (cold stubs there too), so they're ported from the PG impl directly:
  `COALESCE($2, now())` → `COALESCE(?, UNIX_TIMESTAMP())` with an app-bound
  optional ts; `make_interval(hours=>$1)` → a Rust-computed `now - hours*3600`
  cutoff; no `RETURNING` → INSERT-then-re-select. +1 `#[sqlx::test]` (create +
  window + service-filter + cross-org isolation + delete) green on MariaDB.
  PG + SQLite untouched.

## [0.156.53] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `agents` domain** (final scheduler/notifier-tail slice):
  remote probe workers (list / get / create / update / delete / lookup [token
  resolver] / touch_seen), un-stubbing the 7 `StoreAgents` methods on
  `MysqlStore`. Reuses the existing `agents` table (migrations-mysql/0004). No
  `RETURNING` → INSERT-then-re-select; `monitor_count` via `LEFT JOIN monitors …
  GROUP BY` (MySQL 8 functional-dependency); `update` drops the `rows_affected()`
  gate (changed-vs-matched); `delete` clears `monitors.agent_id` in-tx (no
  enforced FK). +2 `#[sqlx::test]` (create/lookup/touch/monitor-count;
  update-clear-location/delete-unassigns) green on MariaDB.
- **MySQL scheduler/notifier dependency tail COMPLETE.** With agents + the prior
  tail slices (digest_buffer .50, maintenance .51, dispatch-path .52), every
  domain the scheduler tick + notifier dispatch touch on the **core monitoring +
  alerting path** is ported. A `mysql://` boot now runs the full loop — probes,
  heartbeat/SLO/detection/telemetry-rule ticks, maintenance windows, monitor-flip
  alert dispatch (resolve channels, silence/dependency suppression, digests) to
  webhook/Slack/email/etc. channels, and agent registration/watchdog — without
  hitting an `unimplemented!()`.

### Known limitation
- Two **feature-conditional** dispatch paths still reach unported cold domains:
  on-call-**targeted** escalation steps (`on_call`) and **web-push** channel
  delivery (`webpush` vapid). They only fire when those features are configured;
  the common alerting path is unaffected. The remaining MySQL stubs are the cold
  management-API domains (status_pages, incidents, error_tracking, profiles, rum,
  api_keys, ingest_keys, on_call, webpush, …) — same shape as the SQLite tier's
  remaining stubs; port on demand. PG + SQLite untouched.

## [0.156.52] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — notifier dispatch-path domains** (scheduler/notifier-tail
  slice 3/N): the 4 per-event dispatch reads + their tables, un-stubbing
  `StoreRouting::resolve_channels_for_monitor`, `StoreSilences::is_silenced`,
  `StoreMonitorGroups::any_parent_down`, and `StoreTemplates::
  get_template_render_strings` on `MysqlStore`. `migrations-mysql/
  0020_dispatch_path.sql` forks the 3 missing tables (monitor_groups[+parent_id],
  monitor_dependencies, silences — notification_templates +
  monitor/group_notifications already existed). Routing runs the same
  `WITH RECURSIVE` folder-ancestor walk as PG/SQLite (MariaDB 10.2+/MySQL 8).
  CRUD for these domains stays stubbed (cold paths) — only the hot dispatch reads
  are wired, mirroring the SQLite dispatch-path slice. **This removes the
  notifier-dispatch panics on a monitor flip**, so a `mysql://` boot with
  monitors + channels now dispatches alerts (resolve channels, suppress silenced
  / dependency-down) without hitting an `unimplemented!()`. +1 `#[sqlx::test]`
  exercising all four reads green on MariaDB. PG + SQLite untouched.

### Known limitation
- The last unported scheduler/notifier-tail domain is **`agents`** (agent
  registration + the watchdog `touch_seen`/`lookup`); an agent-backed monitor's
  ingest on a `mysql://` boot would still hit a stub. Final tail slice.

## [0.156.51] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `maintenance` domain** (scheduler/notifier-tail slice
  2/N): maintenance windows + their monitor sets (list / get / create / update /
  delete / set_active / attach / detach / is_in_active_window /
  transitions_needing_notification / mark_notified_start / mark_notified_end) +
  `migrations-mysql/0019_maintenance.sql`, un-stubbing the 12 `StoreMaintenance`
  methods on `MysqlStore` (the 2 status-page-coupled methods stay stubbed like
  SQLite). `Recurrence::contains` is pure rampart_core, reused verbatim;
  `ON CONFLICT DO NOTHING` → `INSERT IGNORE`; real `ON DELETE CASCADE` FKs on the
  join table; `update` drops the `rows_affected()` gate (MySQL changed-vs-matched
  → trailing `get()`); `set_active` disambiguates a 0-row UPDATE (no-op vs absent)
  with an existence SELECT. **This un-stubs the scheduler tick's maintenance
  check** — combined with `digest_buffer` (v0.156.50), **an idle `mysql://` boot
  is now PANIC-FREE** (verified: built `--features mysql`, booted against MariaDB,
  scheduler + notifier loops ran 30s with zero `unimplemented!()` panics,
  `/healthz` alive). +1 `#[sqlx::test]` (CRUD + attach + active-window +
  transition + no-op-set_active + cross-org) green on MariaDB.

### Known limitation
- A `mysql://` boot **with monitors + channels** can still panic the
  notifier-dispatch / agent-watchdog loops on the last unported domains: routing,
  monitor_groups, silences, templates, agents. Those are the remaining
  scheduler/notifier-tail slices toward full MySQL monitoring parity. PG + SQLite
  unchanged.

## [0.156.50] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `digest_buffer` domain** (scheduler/notifier-tail slice
  1/N toward a panic-free `mysql://` boot). The notifier's durable per-channel
  digest buffer (enqueue / drain_due / take_for_channel / delete_by_ids) +
  `migrations-mysql/0018_digest_buffer.sql` (the table was never created on MySQL
  — a comment referenced it but no DDL), and un-stubs `StoreDigestBuffer` on
  `MysqlStore`. `drain_due` joins `notifications` so each channel flushes on its
  own `digest_window_secs`; a real `ON DELETE CASCADE` FK drops buffered events
  when a channel is deleted. **This removes the notifier digest-flush-timer panic
  on a `mysql://` boot.** +1 `#[sqlx::test]` green on MariaDB. PG + SQLite
  untouched.

## [0.156.49] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — boot flip: `DATABASE_URL=mysql://…` selects `MysqlStore`.**
  `main.rs` gains the `mysql:` scheme branch (alongside `postgres://` and
  `sqlite:`), gated behind a new off-by-default `rampart-api` `mysql` feature
  (`mysql = ["rampart-db/mysql"]`) so the default Postgres build compiles zero
  MySQL code. On a `mysql://` URL it builds `MysqlStore::connect` (no PG pool),
  the leader becomes `Leadership::always()` (no advisory lock without a PG pool),
  and the Postgres-only paths (prune / self-metrics / seed-demo) are skipped —
  same shape as the SQLite flip. **Verified against MariaDB:** the binary built
  `--features mysql` boots on a `mysql://` URL, applies the full migration set,
  and serves `/healthz` (`{"status":"alive","version":"0.156.49"}`); the
  management API + all 20 ported domains' reads/writes work.
- **Known limitation (management-API tier):** the scheduler / notifier background
  loops call Store methods for domains not yet ported to MySQL (maintenance,
  digest_buffer, routing, silences, templates, monitor_groups, agents) → those
  loops `unimplemented!()`-panic in their worker threads (the HTTP server +
  ported domains are unaffected). So MySQL is currently a **management-API +
  telemetry-read tier**; the monitoring/alerting tier needs that scheduler-
  dependency domain tail ported — the same tail SQLite completed (v0.156.11-27)
  before its boot was panic-free. Postgres + SQLite unchanged.

## [0.156.48] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `impl Store for MysqlStore` CAPSTONE.** The object-safe
  `crate::store::Store` super-trait (~46 sub-traits, ~420 methods) is now
  satisfied by MySQL, so `AppState` can hold `Arc<dyn Store>` over **any of the
  three backends** (Postgres / SQLite / MySQL). `rampart-db/src/mysql/store.rs`:
  `MysqlStore { pool }` + `new()` / `connect(url)`. The 20 ported P2 domains
  (settings, orgs, users, sessions, monitors, tags, heartbeats, proxies,
  notifications, delivery_log, escalations, scheduled_reports, audit,
  metric_samples, logs, metric_rules, traces, telemetry_rules, slos, detection)
  delegate to their `crate::mysql::*` free fns; the not-yet-ported domains
  (agents, maintenance, digest_buffer, templates, silences, routing,
  monitor_groups, error_tracking, profiles, rum, status_pages, incidents,
  api_keys, ingest_keys, on_call, …) are `unimplemented!()` stubs that panic if
  hit. **`connect()` sets `sql_mode=STRICT_TRANS_TABLES` per pooled connection**
  (via `after_connect`) so an over-length write errors instead of silently
  truncating — the audit hash chain + detection matching depend on stored ==
  hashed bytes — while keeping MySQL's default backslash-escaping (the detection
  `BodyContains ESCAPE` clause needs it); then runs the `migrations-mysql` set.
  +1 `#[sqlx::test]` keystone: `MysqlStore` is usable as `Arc<dyn Store>`,
  delegated domains (monitors + settings) round-trip through the trait object,
  and the full MySQL migration set applies cleanly. PG + SQLite untouched.
  **Remaining for a `DATABASE_URL=mysql://…` boot: the `mysql:` scheme branch in
  `main.rs` + the `rampart-api` `mysql` feature (mirrors the SQLite boot flip).**

## [0.156.47] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `detection` domain** (SIEM detection rules over the
  `logs` tier + the evaluation tick raising `detection_findings` + the findings
  feed) + `migrations-mysql/0017_detection.sql`. **Completes the MySQL telemetry
  tier** (7/7 domains). The match compiler builds both paths — the flat matcher
  and the Detection-v2 boolean tree (And/Or/Not/Service/MinLevel/BodyRegex/
  BodyContains/Attr) — via `QueryBuilder<MySql>`, binding every leaf (no
  interpolation). MySQL deltas: `body ~* regex` → case-insensitive `LIKE
  CONCAT('%',?,'%')` substring (homelab degrade, same as SQLite); `attributes->>k`
  → `JSON_UNQUOTE(JSON_EXTRACT(attributes,?))` with `COLLATE utf8mb4_bin` on
  attribute equality (case-sensitive, matching SQLite `=`) while body stays
  case-insensitive (matching `~*`); no `RETURNING` → INSERT/UPDATE-then-SELECT
  for insert_finding/ack_finding; `condition` (reserved word) backticked; the
  findings→rules link is a **real `ON DELETE CASCADE` FK** (cascading finding
  cleanup matters for a SIEM) — verified by a cascade-delete test. An adversarial
  multi-agent review (match-compilation / detection-evasion / cross-org-isolation
  lenses, validated live against MariaDB) returned **no blockers**: cross-org
  isolation, the findings lifecycle, ack idempotency, and watermark advancement
  all hold; the known over-match divergences (JSON-null reads as `'null'`,
  accent-insensitive body match, regex→substring) are documented in the module
  note. +2 `#[sqlx::test]` (whole-set fire + threshold gating + feed/ack/cascade/
  cross-org; group_by per-entity + condition tree) green on MariaDB. PG + SQLite
  untouched. **All 7 telemetry domains + the relational subset are now ported;
  next is the `impl Store for MysqlStore` capstone + the `mysql:` boot branch.**

## [0.156.46] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `slos` domain** (service level objectives + budget
  evaluation): list / list_all / get / get_unscoped / create / update / delete /
  compute / trend / list_with_snapshots / evaluate_tick +
  `migrations-mysql/0016_slos.sql`. The budget state machine (`snapshot`,
  `slo_transition`) is pure rampart_core reused verbatim; Monitor-SLI ratios run
  over the ported heartbeats, Metric-SLI ratios over the ported metric_samples.
  MySQL deltas: jsonb `labels @> matcher` containment → one bound
  `JSON_UNQUOTE(JSON_EXTRACT(labels, ?)) = ? COLLATE utf8mb4_bin` per key
  (JSON_EXTRACT returns a quoted scalar → unquote; bin collation = exact match
  like SQLite); `CAST(… AS REAL)` (invalid in MySQL) → `* 1e0` to force DOUBLE;
  `date_bin` → `(ts - since) DIV step`; `SUM(CASE…)` → `CAST(… AS SIGNED)` where
  decoded i64. `update` drops the `rows_affected()` gate (changed-vs-matched).
  +2 `#[sqlx::test]` (monitor SLO compute+fire+no-op-update + metric SLO
  containment with a decoy-label row excluded → exact 90%) green on MariaDB.
  PG + SQLite untouched.

## [0.156.45] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `telemetry_rules` domain** (threshold alert rules over
  the telemetry tiers): list / list_all / get / get_unscoped / create / update /
  delete / evaluate_tick + `migrations-mysql/0015_telemetry_rules.sql`.
  `evaluate_tick` reuses the pure `rule_transition` state machine; `observe`
  computes per-tier aggregates over the now-ported **logs + traces** MySQL
  domains (trace-latency p95 app-side via `p_cont`; trace-error-rate via
  `CAST(SUM(CASE…) AS SIGNED)`; log-volume via `body LIKE CONCAT('%',?,'%')`).
  The error_tracking / profiles / rum tiers aren't forked to MySQL yet → those
  kinds return `None` (no-data → resolve, never a false fire). `update` drops the
  `rows_affected()` gate (MySQL changed-vs-matched) and lets the trailing `get()`
  surface NotFound. +1 `#[sqlx::test]` (CRUD + log-volume fire + not-yet-ported-
  tier no-fire + no-op-update-must-not-404 + cross-org) green on MariaDB.
  PG + SQLite untouched.

## [0.156.44] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `traces` domain** (span storage + trace assembly;
  telemetry foundation 2/2): insert_spans / list_traces / get_trace_spans /
  service_map / operation_stats / operation_trend / prune +
  `migrations-mysql/0014_traces.sql`. MySQL has no `percentile_cont`/`LATERAL`/
  `ARRAY_AGG`, so the four analytic reads fetch span rows and aggregate in Rust
  — incl. a continuous percentile (`p_cont`) matching PG's `percentile_cont`,
  per-trace assembly replacing LATERAL+ARRAY_AGG, list_traces caps the fetch at
  100k spans (identical to SQLite). **`ON CONFLICT(span_id) DO NOTHING` → `INSERT
  IGNORE`** (not `ON DUPLICATE KEY UPDATE col=col`): a duplicate span_id
  contributes 0 to `rows_affected` so the inserted-count stays exact — the
  no-op-UPDATE form returns 1/matched on MariaDB and over-counts retransmits.
  `(received_at-origin)/step` bucket → `DIV`; SMALLINT kind/status decoded
  directly as i16. +1 `#[sqlx::test]` (insert+dedup, waterfall, list/errors/q
  filters, service map p95, operation stats, trend, prune) green on MariaDB.
  PG + SQLite untouched.

## [0.156.43] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `metric_rules` domain** (threshold/anomaly alert rules
  over ingested metrics): list / list_all / get / get_unscoped / create / update
  / delete / evaluate_tick + `migrations-mysql/0013_metric_rules.sql`.
  `evaluate_tick` reuses the pure `rampart_core::metric_rule::rule_transition`
  state machine + the ported `mysql::metric_samples::latest`/`baseline` reads —
  the first MySQL domain to compose another telemetry domain. uuid→CHAR(36),
  jsonb labels + UUID[] channel_ids→LONGTEXT(JSON), double→DOUBLE, ts→BIGINT,
  op→VARCHAR(CHECK), enabled→TINYINT. **`update` does NOT gate on
  `rows_affected()`** — MySQL counts *changed* not *matched* rows, so a no-op
  patch over an existing row reports 0 and the PG/SQLite code would falsely 404;
  the final `get()` surfaces NotFound only when the row is genuinely absent
  (identical result on both engines). +1 `#[sqlx::test]` (CRUD + fire/resolve
  tick + a no-op-update-must-not-404 assertion + cross-org isolation) green on
  MariaDB. PG + SQLite untouched.

## [0.156.42] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `logs` domain** (log storage + filtered reads; the
  telemetry foundation for detection + telemetry_rules): insert_logs /
  query_logs / level_counts / histogram / list_services / prune +
  `migrations-mysql/0012_logs.sql`. MySQL deltas: `UNNEST` insert → per-row tx;
  `date_bin(step, ts, origin)` → `origin + ((ts-origin) DIV step)*step` (integer
  `DIV`, not decimal `/`); `COUNT(*) FILTER` → `CAST(SUM(CASE…) AS SIGNED)`; `||`
  concat → `CONCAT`; the row-value keyset `(ts,id) < (SELECT ts,id …)` ports
  as-is (MySQL row-subquery comparison). **Full-text search degrades to a `LIKE`
  substring match** — like the SQLite tier — because InnoDB `MATCH…AGAINST` is
  word/stopword/min-token based, not substring (a behavior change, not parity);
  a FULLTEXT index is the upgrade path if word-search is wanted. +1 `#[sqlx::test]`
  incl. a keyset-paging assertion that exercises the row-value subquery, green on
  MariaDB. PG + SQLite untouched.

## [0.156.41] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `metric_samples` domain** (externally-pushed metric
  series; the read foundation for metric_rules + slos): insert_many /
  list_series / range_query / baseline / latest / prune_older_than +
  `migrations-mysql/0011_metric_samples.sql`. The **first telemetry-tier MySQL
  domain.** jsonb/canonical-TEXT labels → a **`utf8mb4_bin`** TEXT column so `=`
  and `GROUP BY` are byte-exact — utf8mb4's default collation is case/accent-
  insensitive and would silently merge distinct label sets, so series identity
  must use a binary collation. `(ts/step)*step` bucket → `(ts DIV step)*step`
  integer math; `STDDEV_SAMP` (absent) computed app-side from sum/sum-of-squares/
  count (identical to SQLite); AVG/SUM of a DOUBLE column return DOUBLE so no
  `* 1e0`. PG/SQLite have no PK; MySQL gets a surrogate `id` AUTO_INCREMENT that
  also serves as the same-second `latest` tie-break (replacing SQLite rowid).
  +1 `#[sqlx::test]` incl. a case-sensitive label-identity assertion ("A" ≠ "a")
  green on MariaDB. PG + SQLite untouched.

## [0.156.40] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `heartbeats` analytics tail** (11 fns completing the
  domain): `daily_status` + `daily_status_batch` (uptime ribbon),
  `day_hourly_latency`, `monthly_uptime` + `monthly_uptime_batch`,
  `uptime_pct_batch`, `avg_latency_ms_batch`, `summary_window` (24h dashboard
  rollup), and the SLO walk trio `mtbf_mttr` / `error_budget` /
  `error_budget_burndown`. Faithful mirror of the SQLite reference with the
  MySQL dialect: integer day buckets `ts DIV 86400` (not decimal `/`);
  `strftime('%H'/'%Y-%m', …)` → `HOUR(FROM_UNIXTIME(ts))` /
  `DATE_FORMAT(FROM_UNIXTIME(ts), '%Y-%m')`; `SUM(CASE…)` → `CAST(… AS SIGNED)`;
  `AVG(int)` → `* 1e0` for f64; `MAX(CASE…)` for the BOOL_OR ribbon flags;
  `ROW_NUMBER()` window (aliased derived table) for the latest-status merge. The
  MTBF/MTTR + error-budget computations reuse PG's exact ascending-ts Rust walk
  verbatim (only the query is runtime-checked). +1 `#[sqlx::test]` exercising
  every path over a Up→Down→Up timeline (daily ribbon, monthly %, mtbf/mttr,
  error budget + burndown, summary window, all four batch rollups, hourly
  latency) green on MariaDB. `heartbeats` is now a complete MySQL domain;
  PG + SQLite untouched.

## [0.156.39] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `audit` domain** (the tamper-evident hash chain:
  insert / set_chain_watermark / verify_chain / security_insights / list /
  fetch_since / export_batch) + `migrations-mysql/0010_audit.sql`. The chain
  reuses `crate::audit::chain_hash` **verbatim** so insert + verify feed
  byte-identical inputs. MySQL deltas: no `RETURNING` → `LAST_INSERT_ID()` plus
  an explicitly-bound `ts` (hashed ts == stored ts, no re-select);
  `SUM(CASE…)` → `CAST(… AS SIGNED)`; `date_trunc('hour')` → `(ts DIV 3600)*3600`;
  INET → plain TEXT. **Chain serialization** replaces `pg_advisory_xact_lock`
  (no InnoDB equivalent; `GET_LOCK` is session-scoped → leaks on a pool) with
  **two tx-scoped `FOR UPDATE` locks**: a single-row `audit_chain_lock` to order
  writers (and cover genesis), plus a **locking tip read** so the prev-hash read
  observes the latest *committed* tip regardless of the REPEATABLE-READ snapshot
  — both auto-released on commit, no leak. An adversarial multi-agent review of
  the chain integrity drove the locking-read fix and three watermark/prune
  regression tests (head-truncation-verifies, surviving-row-deletion-detected,
  middle-deletion-detected) mirroring the PG suite. **Honest scope:** the prune
  watermark is stored in plaintext here (as on SQLite — `mysql::settings` has no
  AES-GCM envelope yet), so the PG "sealed watermark can't be forged" guarantee
  does not hold on this backend; backward-linkage + middle/surviving-row tamper
  detection are intact. 4 `#[sqlx::test]` green on MariaDB. 13th MySQL domain;
  PG + SQLite untouched.

## [0.156.38] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL) — `scheduled_reports` domain** (periodic uptime digests:
  list/get/create/update/delete/due/render/mark_sent) +
  `migrations-mysql/0009_scheduled_reports.sql`. MySQL deltas: insert/update-then-
  get (no RETURNING); the cadence `due` CASE uses
  `DATE_FORMAT(FROM_UNIXTIME(last_sent_at), '%Y-%m')` for the monthly bucket;
  `render` reuses the ported `mysql::monitors::list_all` +
  `mysql::heartbeats::uptime_pct`. +1 `#[sqlx::test]` (crud + cadence due-windows
  + render) green on MariaDB. 12th MySQL domain; PG + SQLite untouched.

### Added
- **Multi-DB P2 (MySQL) — `escalations` domain** (policies + the episode state
  machine: 18 fns — list/get/get_unscoped/create/update/delete + open_episode /
  open_episode_for_subject / resolve_subject / ack_episode / list_open /
  list_open_for_org / episode_in_org / open_for_monitor / ack / resolve /
  advance / due). `migrations-mysql/0008_escalations.sql`. **The "one open
  episode per subject" partial unique index (PG/SQLite `WHERE resolved_at IS
  NULL`) has no MySQL equivalent → replaced with a STORED generated column
  `open_key` (= `subject_kind:subject_ref` while open, NULL once resolved) + a
  plain UNIQUE** — so a duplicate open INSERT atomically hits the unique key
  (`Ok(None)`, same semantics) and resolving frees the slot. RETURNING →
  UPDATE/INSERT-then-reselect; the `advance` race-claim re-checks via
  rows_affected. +1 `#[sqlx::test]` (full policy + episode lifecycle incl.
  reopen-after-resolve) green on MariaDB. 11th MySQL domain; PG + SQLite
  untouched.

### Added
- **Multi-DB P2 (MySQL) — `delivery_log` domain.** Append-only channel-send log
  (record / get / list / list_all) + `migrations-mysql/0007_delivery_log.sql`.
  MySQL deltas: BIGSERIAL PK → `BIGINT AUTO_INCREMENT` read back via
  `LAST_INSERT_ID()` (no RETURNING); `record` floors org to the channel's org
  (or Default) in-SQL via a `COALESCE` subquery so system/orphaned rows are
  never NULL. +2 `#[sqlx::test]` (org-floor + get; filter matrix + limit) green
  on MariaDB. 10th MySQL domain. Off by default; PG + SQLite untouched.

### Added
- **Multi-DB P2 (MySQL) — `notifications` (channels) domain.** Full channel CRUD
  (list / list_all / get / get_unscoped / create / update / delete /
  counts_per_monitor / attach / detach / for_monitor / mark_fired) +
  `migrations-mysql/0006_notifications.sql` (notifications + notification_templates
  + monitor_notifications + group_notifications + monitor_notification_excludes).
  Channel `config` sealed via `crate::secrets::seal` on write, re-opened on every
  read (the #112 invariant); reuses the dialect-neutral clamp helpers + the
  double-Option `template_id`/quiet-hours merge; tag hydration through
  `mysql::tags`. MySQL deltas: insert/update-then-get (no RETURNING),
  `ON DUPLICATE KEY` attach. +3 `#[sqlx::test]` (clamps + seal/open + cross-org;
  enum round-trip; attach/for_monitor/counts/tag-hydration) green on MariaDB.
  9th MySQL domain. Off by default; PG + SQLite untouched.

### Added
- **Multi-DB P2 (MySQL) — `proxies` domain.** Outbound proxy configs for probe
  routing (list / get / get_unscoped / create / delete / set_active);
  `migrations-mysql/0005_proxies.sql`. `auth` derived from username/password on
  create; insert-then-get (no RETURNING). 8th MySQL domain; suite green on
  MariaDB. Off by default; PG + SQLite untouched.

### Added
- **Multi-DB P2 (MySQL) — `heartbeats` core domain.** The probe time-series
  writer (`insert_many`, `ON DUPLICATE KEY` idempotent on (monitor_id, ts)) +
  the history feeds (`recent_for_monitor` / `_before` / `range_for_monitor` /
  `recent_per_monitor`) + the trailing-window reads (`uptime_pct` /
  `current_slo_uptime_pct` / `avg_latency_ms`) the scheduler + monitor detail
  need (the heartbeats table was already created in 0004). Two MySQL aggregate
  gotchas pinned down on real MariaDB: **`SUM(CASE…)` returns DECIMAL → wrap in
  `CAST(… AS SIGNED)`** to decode as i64, and **`AVG(INT)` returns DECIMAL →
  `* 1e0`** to force DOUBLE for f64; the `recent_per_monitor` derived table also
  needs a `) AS sub` alias (MySQL requires it). The dashboard rollups (daily/
  monthly buckets, mtbf/mttr, error budget, batch variants — they need the
  `ts DIV 86400` / `DATE_FORMAT(FROM_UNIXTIME …)` bucket translations) are a
  follow-up `heartbeats-analytics` slice. Full `mysql::` suite 19/19 green on
  MariaDB. Off by default; PG + SQLite untouched.

### Added
- **Multi-DB P2 (MySQL) — core monitoring: `monitors` + `tags` domains.** The
  big coupled slice: `mysql::monitors` (28 fns: CRUD, partial `update` with
  double-Option clears, `bulk_edit`/`bulk_edit_preview`, push-token + run
  lifecycle, cert/SLO state, `set_active_by_tag`, `list_stale_agent_monitors`,
  `public_fields_batch`) + `mysql::tags` (11 fns incl batch hydrators) +
  `migrations-mysql/0004_monitoring.sql` (monitors + heartbeats + agents + tags
  + the 3 tag join-tables, one migration because the domains are mutually
  dependent: monitors hydrate tags, monitor_tags FKs monitors, the stale-agent
  watchdog JOINs agents + heartbeats). MySQL deltas: strict integer typing (all
  Monitor int fields i32 → INT, ts → BIGINT, bools → TINYINT decoded as i64);
  no `RETURNING` → insert-then-get; `unixepoch()` → `UNIX_TIMESTAMP()`;
  `ON CONFLICT DO NOTHING` → `ON DUPLICATE KEY`; UPDATE existence gated on a
  SELECT (MySQL reports changed not matched rows). **Verified on real MariaDB:
  the full `mysql::` suite is 18/18 green** (settings/orgs/sessions/users +
  monitors/tags). Off by default; PG + SQLite untouched.

### Added
- **Multi-DB P2 (MySQL) — auth core: `users` + `sessions` domains.**
  `mysql::users` (20 fns: accounts / RBAC role / TOTP-2FA lockout / prefs / GDPR
  anonymize, role-mirror onto Default-org membership + session revocation on
  privilege/2FA downgrade) and `mysql::sessions` (9 fns) +
  `migrations-mysql/0003_sessions.sql`. MySQL dialect deltas handled: no
  `RETURNING` → insert-then-get; `unixepoch()` → `UNIX_TIMESTAMP()`; `||` →
  `CONCAT` (anonymize); upsert → `ON DUPLICATE KEY UPDATE`; and the TOTP-lockout
  UPDATE orders the `totp_locked_until` CASE **before** the counter increment
  because MySQL evaluates `SET` left-to-right over already-updated values (unlike
  the SQL-standard all-old-values that PG/SQLite use) — otherwise the lock
  threshold would double-count. **Verified on a real MariaDB**: the full
  `mysql::` suite (settings + orgs + sessions + users) is 12/12 green locally,
  not just compile-checked. Off by default; PG + SQLite untouched.

### Added
- **Multi-DB P2 (MySQL) — identity domain: `orgs` + the identity migration.**
  `migrations-mysql/0002_identity.sql` forks the users / organizations /
  org_members tenancy core (CHAR(36) uuids, BIGINT unix timestamps with
  `DEFAULT (UNIX_TIMESTAMP())`, TINYINT bools, utf8mb4 case-insensitive UNIQUE
  email, REGEXP slug CHECK, seeded Default org). `mysql::orgs` ports the full
  free-fn surface (create / get / get_by_slug / update / list_for_user /
  upsert_member / member_role / list_members(_detailed) / remove_member /
  count_admins / create_with_owner). MySQL deltas vs SQLite: no `RETURNING` →
  app-side UUID PK + INSERT-then-SELECT; `update` gates existence on a SELECT
  first (MySQL UPDATE reports *changed* not *matched* rows, so a no-op rename
  isn't a false NotFound); upsert is `ON DUPLICATE KEY UPDATE role = VALUES(role)`.
  Shared dialect helpers (oid/uid/ts/role) added to `mysql::mod`. +5
  `#[sqlx::test]` (run by the `backend-mysql` CI service). `users` domain next.
  Off by default; PG + SQLite untouched.

## [0.156.29] — 2026-06-23

### Added
- **Multi-DB P2 (MySQL/MariaDB) — P0 toolchain spike.** Mirrors the SQLite
  P1-0 spike: a new off-by-default `mysql` cargo feature on `rampart-db`
  (`mysql` is already in the workspace sqlx features), `rampart_db::mysql`
  module gated behind it, `migrations-mysql/0001_settings.sql`, and a
  `mysql::settings` domain (get / put / delete) whose `put` exercises the
  `INSERT … ON DUPLICATE KEY UPDATE` upsert dialect (the PG `ON CONFLICT` /
  SQLite `excluded` equivalent). Runtime-checked `sqlx::query`/`query_as` so it
  builds under `SQLX_OFFLINE` alongside the PG cache; a new `backend-mysql` CI
  job spins up a `mysql:8` service container and runs the `#[sqlx::test]`
  settings round-trip for real (no local MySQL, so the test is CI-only). The
  module doc records the P2 dialect conventions (uuid→CHAR(36), ts→BIGINT,
  JSON→LONGTEXT or native JSON+JSON_EXTRACT, no RETURNING → app-side UUID PK +
  INSERT-then-SELECT, no array binds → bound `IN (?,…)`, GET_LOCK leader). Off
  by default; the PG + SQLite builds are untouched. Real domains + the full
  `impl Store for MysqlStore` + boot flip follow the SQLite playbook.

## [0.156.28] — 2026-06-23

### Added
- **Multi-DB P1 — the boot flip: Rampart runs on SQLite.** `DATABASE_URL`'s
  scheme now selects the backend at boot — `postgres://…` → PgStore (+ pool, the
  reference build); `sqlite:…` → SqliteStore (single-binary / homelab tier).
  Verified end-to-end: built with `--features sqlite`, booted against a
  `sqlite:///…` file (20 migrations applied, scheduler + notifier + SIEM loops
  spawned on the object-safe seam, HTTP up, `/healthz` → alive) with no panic.
  Mechanics: new off-by-default `sqlite` cargo feature on `rampart-api`
  (`= ["rampart-db/sqlite"]`) so the default build stays Postgres-only with zero
  SQLite code; `AppState.pool` is now `Option<DbPool>` (`None` on SQLite) with a
  new `with_scheduler_store(Arc<dyn Store>, …)` constructor; leader election is
  Postgres-advisory-lock on PG and `Leadership::always()` on SQLite (single
  binary); the prune loop + self-metrics + `seed-demo`/`reset-password`
  subcommands are Postgres-only (clean bail / skip on SQLite). CI gains a
  `clippy -p rampart-api --features sqlite` lane. Core monitoring + alerting work
  on SQLite; the residual Postgres-only telemetry-ingest / management endpoints
  (the ~181 cold SqliteStore stubs) `expect()` the pool and are out of the
  supported SQLite surface until ported on demand.

## [0.156.27] — 2026-06-23

### Added
- **Multi-DB P1 domain-port tail: SQLite notifier dispatch path.** The notifier's
  per-event dispatch hit 4 still-stubbed SqliteStore reads that would panic a
  sqlite boot on the first alert: `resolve_channels_for_monitor` (routing union),
  `is_silenced` (silences), `get_template_render_strings` (templates), and
  `any_parent_down` (monitor_groups dependency suppression). All 4 now real.
  `migrations-sqlite/0020_dispatch_path.sql` forks the 3 missing tables
  (monitor_groups + parent_id folder tree, monitor_dependencies, silences;
  uuid→TEXT, ts→INTEGER). `routing::resolve_channels_for_monitor` runs the same
  `WITH RECURSIVE` folder-ancestor walk as PG (SQLite supports it) and reuses the
  `sqlite::notifications` row decoder. CRUD for these domains stays stubbed (cold
  management-API paths) — only the hot dispatch reads are wired. +1 `#[sqlx::test]`
  (empty boot case + attach→resolve roundtrip + global silence). clippy + fmt
  green; PG untouched.

### Changed
- **Multi-DB P1 seam-plumbing slice C: rampart-scheduler fully off `&DbPool`
  onto the `Store` seam.** All 55 of the scheduler's own domain calls
  (`rampart_db::<domain>::fn(&self.pool, …)` across the escalation tick, the
  metric-rule / SLO / telemetry-rule / detection evaluation ticks, the
  maintenance + scheduled-report + audit-chain periodic checks, the probe path
  run_once / probe_once / probe_with_retries / push_heartbeat, and the writer
  path flush / check_slo_breaches / fire_result_webhooks) now go through
  `Arc<dyn Store>`. The `Scheduler` struct drops its `pool: DbPool` field
  entirely — `store: Arc<dyn Store>` is the only DB handle; `new(pool)` builds a
  PgStore from it, `with_notifier(store, notifier)` takes the store directly.
  Probe tasks + the writer task + the per-heartbeat cert/webhook spawns own
  `Arc<dyn Store>` clones. `with_notifier` lost its `pool` arg (main.rs updated).
  Full-workspace `clippy --all-targets -D warnings` + fmt green; scheduler +
  notifier unit tests pass. PG behavior identical. With this, scheduler +
  notifier + siem all run on the seam — only seed/import + the residual api
  pool() sites + the main.rs backend-select flip remain.

### Changed
- **Multi-DB P1 seam-plumbing slice B2: rampart-notifier core off `&DbPool`
  onto the `Store` seam.** The whole notifier service (service.rs ~18 fns:
  dispatch_one / send_event_to_channel / fire_escalation_step / dispatch_one's
  digest flush / resend_delivery / dispatch_error_alert / send_system_email /
  fan_out_maintenance_subscribers / render_digest …), the channel dispatch
  entrypoint (`channels::dispatch`) and the Web Push fan-out (`webpush::send_all`)
  now take `&dyn Store` / `Arc<dyn Store>` instead of a concrete pool. `dispatch_one`
  takes an owned `Arc<dyn Store>` so its per-channel `tokio::spawn` tasks get a
  cheap clone. Web Push composes the existing `get_vapid_keys` / `set_vapid_keys`
  + its own generator (the closure-based `get_or_create_vapid` free fn isn't
  object-safe). `NotifierService` holds `store: Arc<dyn Store>`. Callers updated:
  the 6 rampart-api routes (delivery_log retry, notifications/monitors send-test,
  scheduled-reports, rum + error-ingest alert spawns) now pass `state.store()`;
  the scheduler gains an `Arc<dyn Store>` (used only for its 7 cross-crate
  notifier calls — its own domain reads stay on the pool until slice C);
  main.rs builds one `Arc<dyn Store>` and shares it across notifier/scheduler/siem.
  68 notifier unit tests pass; full workspace `clippy --all-targets -D warnings`
  + fmt green. PG behavior identical; off-by-default sqlite untouched.

## [0.156.24] — 2026-06-23

### Changed
- **Multi-DB P1 seam-plumbing slice B1: rampart-notifier `siem` export off
  `&DbPool` onto `Arc<dyn Store>`.** The SIEM/syslog forward-tail loop
  (`siem::run_loop` + load_config / cursor / findings_cursor / tick) now takes
  the object-safe `Store` seam instead of a concrete pool — all 4 of its DB
  touches (settings get/put, `fetch_audit_since`, `fetch_detection_findings_since`)
  were already trait-covered, so this is pure plumbing. main.rs builds the store
  from the pool (no I/O) and hands it to the loop. The 68 notifier unit tests
  (CEF/LEEF/syslog formatting — all pool-free) pass unchanged; PG behavior
  identical. First running component fully on the seam; service.rs + channels +
  scheduler follow.

### Added
- **Multi-DB P1 seam-plumbing slice A: `StoreDigestBuffer` trait + SQLite
  `digest_buffer` domain.** Foundation for migrating rampart-notifier off the
  concrete `&DbPool`: a 6-reader mapping workflow over scheduler / notifier /
  seed / import / api / boot found the notifier's per-channel digest buffer is
  the only consumer with NO Store-trait coverage (it called
  `rampart_db::digest_buffer::` free fns directly). New `StoreDigestBuffer`
  sub-trait (enqueue_digest / drain_due_digests / take_digest_for_channel /
  delete_digest_by_ids) added to the `Store` super-trait, impl'd on PgStore
  (delegates) and SqliteStore (new `sqlite::digest_buffer` — migration-free; the
  table was pre-created in `0007_notifications.sql`). +1 `#[sqlx::test]`
  (enqueue → window-gated drain_due → take → scoped delete). Additive, zero
  callers yet (the notifier migration consumes it next); PG behavior unchanged.
  The vapid case needs no new method — Store callers compose the existing
  get/set_vapid_keys with their own generator (the `get_or_create_vapid` closure
  is intentionally non-object-safe). Off-by-default `sqlite`; PG untouched.

## [0.156.22] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `detection` domain (un-stubs
  `StoreDetection`) — the LAST scheduler-dependency domain.**
  `migrations-sqlite/0019_detection.sql` (forks PG 0090/0091/0103/0104/0105 +
  org_id: `detection_rules` + `detection_findings`, uuid→TEXT, bool→0/1,
  UUID[] channel_ids→JSON, jsonb condition→TEXT, ts→INTEGER) +
  `sqlite::detection`: the full free-fn surface — regex_is_valid / list /
  list_all / get / get_unscoped / create / update / delete / preview /
  evaluate_tick / has_recent_finding / list_findings(_for_org) /
  finding_in_org / open_count / fetch_since / ack_finding. The match layer
  ports both paths via `QueryBuilder<Sqlite>`: the legacy flat-field match and
  the Detection v2 boolean condition tree, binding every leaf (no
  interpolation). Dialect degrades: `body ~* regex` → case-insensitive `LIKE`
  substring (no SQLite regex; `regex_is_valid` always accepts), `attributes->>k`
  → `json_extract`, the per-entity ordered `array_agg(...)[1]` newest-sample is
  aggregated app-side, watermarks are whole-second `unixepoch()` (documented
  homelab edge; strict `>` kept so matches never double-count). +2
  `#[sqlx::test]` (whole-set fire + threshold gating + findings feed/ack/
  cross-org; group_by per-entity + boolean condition tree). **All 9
  scheduler-dependency domains are now ported** — the next phase is
  seam-plumbing (scheduler/notifier/seed/import onto `&dyn Store`) + the main.rs
  `RAMPART_DB_URL=sqlite` flip (the bootable milestone). Off-by-default
  `sqlite`; PG untouched.

---

## [0.156.21] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `telemetry_rules` domain (un-stubs
  `StoreTelemetryRules`).** `migrations-sqlite/0018_telemetry_rules.sql`
  (kind app-validated, op CHECK ported, UUID[]→JSON, ts→INTEGER) +
  `sqlite::telemetry_rules`: list / list_all / get / get_unscoped / create /
  update / delete / evaluate_tick. Reuses the pure `rule_transition` state
  machine. `observe` computes the per-tier aggregate: **log_volume**,
  **trace_latency** (p95 app-side via `p_cont`) and **trace_error_rate** are
  real (logs+traces ported); the **error_rate / profile_samples / rum_lcp_p75**
  tiers return `None` (no-data → resolve, never a false fire) until
  error_tracking / profiles / rum are forked — documented so the gap is visible
  rather than a runtime "no such table". +1 `#[sqlx::test]` (log_volume fire +
  not-yet-ported tier no-fires + CRUD/cross-org). **Boot-wiring: only
  `detection` remains** of the scheduler-dep domains. Off-by-default `sqlite`;
  PG untouched.

---

## [0.156.20] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `traces` domain (un-stubs `StoreTraces`).**
  Second telemetry-foundation domain (completes logs+traces; unblocks the
  remaining detection + telemetry_rules). `migrations-sqlite/0017_traces.sql`
  (span_id PK, ns→INTEGER, double→REAL, jsonb→TEXT) + `sqlite::traces`:
  insert_spans (per-row tx, `ON CONFLICT(span_id) DO NOTHING` dedup) /
  list_traces / get_trace_spans / service_map / operation_stats /
  operation_trend / prune. **SQLite has no `percentile_cont` / `LATERAL` /
  `ARRAY_AGG`, so the four analytic reads fetch span rows and aggregate in
  Rust** — a continuous percentile (`p_cont`, matching `percentile_cont`),
  per-trace assembly (root span, services, error counts, `(received_at,
  trace_id)` keyset), service-dependency edges (p95 callee latency), and the
  per-operation APM rollup (p50/p95/p99/avg/max). +1 `#[sqlx::test]` (insert+
  dedup, waterfall, errors_only/q filters, service_map p95, operation_stats
  error-rate, operation_trend, prune). **Boot-wiring: logs + traces done — the
  telemetry foundation is complete**; detection + telemetry_rules remain.
  Off-by-default `sqlite`; PG untouched.

---

## [0.156.19] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `logs` domain (un-stubs `StoreLogs`).**
  First of the telemetry foundation (unblocks telemetry_rules + detection).
  `migrations-sqlite/0016_logs.sql` (uuid→TEXT, smallint→INTEGER, jsonb attrs→
  TEXT, ts/received_at→INTEGER; recent/service/org/trace indexes) +
  `sqlite::logs`: insert_logs / query_logs / level_counts / histogram /
  list_services / prune. PG-isms translated: `UNNEST`→per-row tx;
  `make_interval(hours=>?)`→`unixepoch() - hours*3600`; `date_bin`→
  `origin + ((ts-origin)/step)*step`; `COUNT(*) FILTER`→`SUM(CASE …)`; row-value
  keyset `(ts,id) < (subquery)` ports as-is.
- **NOTE — log full-text search degraded on SQLite.** PG's `body_tsv @@
  websearch_to_tsquery('english', …)` (generated tsvector + GIN) has no SQLite
  equivalent, so the SQLite backend matches `body LIKE '%query%'` — substring
  only, no phrase/OR/negation. Acceptable for the single-binary homelab tier;
  documented in the module. +1 `#[sqlx::test]` (insert + service/severity/body/
  trace filters + level_counts + histogram error-split + prune). **Boot-wiring:
  logs done; traces next, then telemetry_rules + detection.** Off-by-default
  `sqlite`; PG untouched.

---

## [0.156.18] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `audit` domain (un-stubs `StoreAudit`).**
  Eighth boot-wiring slice — the tamper-evident audit log.
  `migrations-sqlite/0015_audit.sql` (global, no org_id; BIGSERIAL→INTEGER PK,
  INET→TEXT, jsonb payload→TEXT, ts→INTEGER) + `sqlite::audit`: insert /
  set_chain_watermark / verify_chain / security_insights / list / fetch_since /
  export_batch. **The hash chain reuses `crate::audit::chain_hash` verbatim**
  (promoted to `pub(crate)`) so insert and verify feed byte-identical inputs —
  the chain is self-consistent within SQLite; IP is stored+hashed+displayed as
  the plain address. PG-isms translated: `pg_advisory_xact_lock` dropped (a
  SQLite write tx already serializes appends), `COUNT(*) FILTER`→`SUM(CASE …)`,
  `make_interval(hours=>?)`→`unixepoch() - hours*3600`,
  `date_trunc('hour')`→`(ts/3600)*3600`, `host(ip_addr)`/INET→TEXT. +1
  `#[sqlx::test]` proving a clean 4-row chain verifies, security-insights
  aggregates, list/fetch_since, **and tamper detection** (editing row 2 →
  `first_bad_id = 2`). **Boot-wiring 8/9**; only the telemetry pair left
  (detection + telemetry_rules, both needing logs+traces forked first).
  Off-by-default `sqlite`; PG untouched.

---

## [0.156.17] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `slos` domain (un-stubs `StoreSlos`).**
  Seventh boot-wiring slice. `migrations-sqlite/0014_slos.sql` (sli_kind /
  objective / window CHECKs ported; jsonb labels→TEXT, UUID[]→JSON, ts→INTEGER)
  + `sqlite::slos`: list / list_all / get / get_unscoped / create / update /
  delete / compute / trend / list_with_snapshots / evaluate_tick. The budget
  state machine (`snapshot` / `slo_transition`) is reused from rampart_core;
  Monitor-SLI ratios run over the ported heartbeats, Metric-SLI over the ported
  metric_samples. PG-isms translated: `COUNT/SUM(...) FILTER` → `SUM(CASE …)`;
  `date_bin(make_interval, ts, origin)` → integer bucket `(ts - since)/step`;
  and — the hard one — **jsonb `labels @> matcher` containment → one bound
  `json_extract(labels, ?) = ?` per matcher key** (Prometheus labels are
  string→string). +2 `#[sqlx::test]` (Monitor SLO compute/fire + Metric SLO
  containment with a decoy-label row correctly excluded). **Boot-wiring 7/9**;
  3 left (telemetry_rules+logs/traces, detection, audit). Off-by-default
  `sqlite`; PG untouched.

---

## [0.156.16] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `metric_rules` domain (un-stubs
  `StoreMetricRules`).** Sixth boot-wiring slice; unblocked by metric_samples.
  `migrations-sqlite/0013_metric_rules.sql` (op CHECK incl `anomaly`, jsonb
  labels→TEXT, `UUID[]` channel_ids→JSON, escalation FK, ts→INTEGER) +
  `sqlite::metric_rules`: list / list_all / get / get_unscoped / create / update
  / delete / evaluate_tick. `evaluate_tick` reuses the pure
  `rule_transition` state machine + `RuleOp::breached`/`anomaly_breached` and
  delegates sample reads to the ported `sqlite::metric_samples`. +1
  `#[sqlx::test]` (CRUD + a full fire→resolve evaluation cycle). **Boot-wiring
  6/9**; 3 left (slos, telemetry_rules+logs/traces, detection, audit).

### Fixed
- **SQLite `metric_samples::latest` tie-breaks by `rowid DESC`.** SQLite `ts` is
  second-granular (PG was microsecond), so same-second samples made
  `ORDER BY ts DESC LIMIT 1` ambiguous — it could return a stale value and wedge
  a metric rule firing. Newest-insert now wins. (Off-by-default `sqlite`
  feature; PG path unaffected.)

---

## [0.156.15] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `metric_samples` domain (un-stubs
  `StoreMetricSamples`).** Fifth boot-wiring slice + the telemetry-read
  foundation that unblocks metric_rules + slos. `migrations-sqlite/
  0012_metric_samples.sql` (jsonb labels → canonical-JSON TEXT, double→REAL,
  ts→INTEGER) + `sqlite::metric_samples`: insert_many / list_series /
  range_query / baseline / latest / prune_older_than. PG-isms translated:
  `UNNEST` insert → per-row tx with one stamped `now`; bucket
  `TO_TIMESTAMP(FLOOR(EXTRACT(EPOCH …)/step)*step)` → `(ts/step)*step` integer
  math; **`STDDEV_SAMP` (absent on SQLite) → sample stddev computed app-side**
  from `SUM(value)`/`SUM(value*value)`/`COUNT`; jsonb `labels =` → canonical
  TEXT equality (serde_json sorts keys by default, so it matches PG's semantic
  equality). +1 `#[sqlx::test]` (insert/series/latest/baseline-math/range/prune
  + label-mismatch). **Boot-wiring 5/9**; 4 left (metric_rules + slos now
  unblocked, then telemetry_rules+logs/traces, detection, audit). Off-by-default
  `sqlite`; PG untouched.

---

## [0.156.14] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `escalations` domain (un-stubs
  `StoreEscalations`).** Fourth boot-wiring slice; standalone (no cross-domain
  reads). `migrations-sqlite/0011_escalations.sql` (`escalation_policies` +
  `escalation_episodes`; jsonb steps→TEXT, ts→INTEGER, the "one open episode per
  subject" partial unique index ported verbatim) + `sqlite::escalations` for the
  full 18-fn surface — policy CRUD + the episode state machine (open_episode /
  open_episode_for_subject / resolve_subject / ack_episode / list_open /
  list_open_for_org / episode_in_org / open_for_monitor / ack / resolve /
  advance / due). Partial-target `ON CONFLICT(subject_kind, subject_ref) WHERE
  resolved_at IS NULL DO NOTHING` ports as-is (SQLite 3.35+); `next_due` is the
  pure rampart_core logic; `NOW()`→`unixepoch()`. +1 `#[sqlx::test]` (policy CRUD
  + subject-episode open/idempotency/advance/resolve + cross-org gate).
  **Boot-wiring 4/9** (proxies, scheduled_reports, maintenance, escalations); 5
  left (audit; detection; and the telemetry-coupled metric_rules+metric_samples,
  slos, telemetry_rules+logs/traces). Off-by-default `sqlite`; PG untouched.

---

## [0.156.13] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `maintenance` domain (un-stubs the
  scheduler-relevant `StoreMaintenance` methods).** Third boot-wiring slice.
  `migrations-sqlite/0010_maintenance.sql` (`maintenance_windows` +
  `maintenance_window_monitors`; recurrence jsonb→TEXT, timestamps→INTEGER,
  range CHECK) + `sqlite::maintenance` for the 12 self-contained fns — list /
  get / create / update (double-Option description clear) / delete / set_active
  / attach / detach / is_in_active_window / transitions_needing_notification /
  mark_notified_start / mark_notified_end. The recurrence evaluation
  (`Recurrence::contains`) is the pure rampart_core logic, reused verbatim;
  `= ANY` edge hydration → bound `IN (?,…)`. Covers all 4 scheduler maintenance
  deps. DEFERRED (still stubbed — couple to the not-yet-ported `status_pages`
  tables): `confirmed_subscriber_emails_for_monitors` +
  `public_maintenance_for_status_page`. +1 `#[sqlx::test]`. **Boot-wiring 3/9
  scheduler-dep domains done** (proxies, scheduled_reports, maintenance); 6 left
  (audit, detection, escalations, metric_rules+metric_samples, slos,
  telemetry_rules+logs/traces). Off-by-default `sqlite` feature; PG untouched.

---

## [0.156.12] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `scheduled_reports` domain (un-stubs
  `StoreScheduledReports`).** Second boot-wiring slice. `migrations-sqlite/
  0009_scheduled_reports.sql` (TEXT[] recipients → JSON array TEXT, timestamps
  → INTEGER unix-seconds) + `sqlite::scheduled_reports` mirroring the PG surface
  — list / get / create / update / delete / due / render / mark_sent. The PG
  `due` cadence CASE (`interval` + `date_trunc('month')`) becomes unix-second
  arithmetic + `strftime('%Y-%m', …, 'unixepoch')`; `render` reuses the
  already-ported `sqlite::monitors::list_all` + `sqlite::heartbeats::uptime_pct`
  and the dialect-neutral `cadence_window_seconds` from the PG module so the
  windows can't drift. `StoreScheduledReports` now delegates (was stub). +1
  `#[sqlx::test]`. **7 scheduler-dep domains still stubbed** (audit, detection,
  escalations, maintenance, metric_rules+metric_samples, slos, telemetry_rules)
  — note `metric_rules.evaluate_tick` couples to `metric_samples`, so those port
  together. Off-by-default `sqlite` feature; PG build untouched.

---

## [0.156.11] — 2026-06-23

### Added
- **Multi-DB P1 boot-wiring: SQLite `proxies` domain (un-stubs `StoreProxies`).**
  First of the boot-wiring slices — the scheduler/probe path calls 13 domains,
  9 of which were `unimplemented!()` stubs in `SqliteStore` (so a sqlite boot
  would panic on the first tick). This forks `proxies` to SQLite
  (`migrations-sqlite/0008_proxies.sql` + `sqlite::proxies`: list / get /
  get_unscoped / create / delete / set_active — protocol+port CHECKs ported,
  `auth` derived from username/password, password stored verbatim as in PG) and
  swaps the `StoreProxies` stub for real delegation. +1 `#[sqlx::test]`. **8
  domains still stubbed for full scheduler boot** (audit, detection, escalations,
  maintenance, metric_rules, scheduled_reports, slos, telemetry_rules) — each a
  following slice. Off-by-default `sqlite` feature; PG build untouched.

---

## [0.156.10] — 2026-06-23

### Added
- **Multi-DB P1 CAPSTONE: `impl Store for SqliteStore`.** The object-safe
  `Store` super-trait (46 sub-traits, ~421 methods) is now satisfied by a
  SQLite backend, so `AppState` could hold `Arc<dyn Store>` over Postgres OR
  SQLite. New `rampart-db/src/sqlite/store.rs`: `SqliteStore { pool: SqlitePool }`
  with `new(pool)` and `connect(url)` (sets per-connection `foreign_keys(true)`
  — off by default on SQLite — and runs the `migrations-sqlite` set). The 10
  domains ported in P1 (settings, orgs, users, sessions, monitors, heartbeats,
  tags, agents, notifications, delivery_log) **delegate** to their
  `crate::sqlite::*` free functions; the remaining 37 are `unimplemented!()`
  stubs that panic if hit — they light up as each domain is forked. 2
  `#[sqlx::test]`/`#[tokio::test]` prove `Arc<dyn Store>` round-trips a delegated
  domain and that `connect` migrates a fresh DB. Added `delete_setting` to
  `sqlite::settings` to complete `StoreSettings`. New CI lane **`backend ·
  sqlite backend`** runs the `sqlite`-feature clippy (`-D warnings`) + the
  `sqlite::` test suite (no DB service — `#[sqlx::test]` spins per-test SQLite
  DBs). **32 sqlite tests.**
- **NOT YET wired into boot:** `AppState` still holds a `PgPool` that the
  not-yet-seamed callers (scheduler / notifier / seed) use directly, so a true
  `RAMPART_DB_URL=sqlite` end-to-end boot needs that pool abstracted first (a
  follow-on slice). `SqliteStore` + `connect` exist, compile, and are tested now.

---

## [0.156.9] — 2026-06-23

### Added
- **Multi-DB P1: SQLite monitor `bulk_edit` + the full heartbeat analytic
  surface.** Closes the last deferred items inside the built SQLite domains.
  `sqlite::monitors`: `bulk_edit` / `bulk_edit_preview` (one transaction,
  all-or-nothing; unknown/cross-org ids skipped+counted; scalar columns COALESCE,
  group/tags only on explicit supply; pre-edit `priors` returned for an inverse
  undo — SQLite has no `FOR UPDATE` but a write tx serializes the DB).
  `sqlite::heartbeats`: the history feeds (`recent_for_monitor_before`,
  `range_for_monitor`, `recent_per_monitor`) and every analytic —
  `current_slo_uptime_pct`, `avg_latency_ms`, `daily_status`, `day_hourly_latency`,
  `monthly_uptime`, `summary_window`, `mtbf_mttr`, `error_budget`,
  `error_budget_burndown`, and the four batch rollups (`uptime_pct_batch`,
  `avg_latency_ms_batch`, `daily_status_batch`, `monthly_uptime_batch`).
  PG-isms are translated for SQLite: `COUNT(*) FILTER`→`SUM(CASE)`,
  `BOOL_OR`→`MAX(CASE)`, `date_trunc('day')`→the `ts/86400` whole-day bucket,
  `date_trunc('month')`/`EXTRACT(HOUR)`→`strftime`, `ARRAY_AGG(…ORDER BY)[1]`→a
  `ROW_NUMBER()` window merged in Rust, `= ANY`→a bound `IN(?,…)`. The MTBF/MTTR
  + error-budget timeline walks reuse PG's exact ascending-ts Rust logic. +2
  `#[sqlx::test]` (analytics walk/aggregate math + batch mirrors; bulk_edit apply/
  preview/undo-priors/cross-org). **SQLite monitors + heartbeats now feature-
  complete vs PG; 28 sqlite tests.** Off-by-default `sqlite` feature; PG build
  untouched.

---

## [0.156.8] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `notifications` (channels) + `delivery_log` domains.**
  New `migrations-sqlite/0007_notifications.sql` (notification_templates,
  notifications, monitor_notifications, group_notifications,
  monitor_notification_excludes, digest_buffer, delivery_log — dialect-mapped:
  uuid→TEXT, ts→INTEGER unix-seconds, bool→0/1, jsonb→TEXT, channel_kind→
  app-validated TEXT, per-org template-name unique index, delivery_log
  `BIGSERIAL`→`INTEGER PRIMARY KEY`). `sqlite::notifications` mirrors the full PG
  free-fn surface — list / list_all / get / get_unscoped / create / update
  (read-modify-write preserving the `double_option` template/quiet-hours
  semantics) / counts_per_monitor / delete / attach / detach / for_monitor /
  mark_fired. **Channel `config` is sealed by `crate::secrets::seal` on write and
  re-opened by `crate::secrets::open` on EVERY read** (the row helper centralizes
  it so no path can repeat the #112 decrypt-on-fanout bug); clamps + structs are
  reused from the PG module (clamp helpers promoted to `pub(crate)`) so behavior
  can't drift across backends. `sqlite::delivery_log` mirrors record / get / list
  (keyset + nullable filters) / list_all, with the same in-SQL org floor
  (`COALESCE((SELECT org_id FROM notifications WHERE id = ?), Default)`). Tags
  hydrate via the existing `sqlite::tags::hydrate_for_channels`. +5 `#[sqlx::test]`
  (CRUD/clamps/double-option/cross-org, enum round-trip, attach/for_monitor/
  counts/tags/mark_fired, delivery org-floor + list filter matrix). **SQLite
  domains now 10: settings, orgs, users, sessions, monitors, heartbeats, tags,
  agents, notifications, delivery_log (26 tests).** Deferred: the routing
  resolver (`resolve_channels_for_monitor` needs the not-yet-forked
  `monitor_groups`) and notification_template / digest_buffer CRUD. Off-by-default
  `sqlite` feature; PG build untouched.

---

## [0.156.7] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `agents` domain + stale-agent watchdog.** New
  `migrations-sqlite/0006_agents.sql` (`agents` table: TEXT uuids, INTEGER
  unix-second timestamps, the `token_hash` UNIQUE index that backs the lookup
  hot path, org-scoped). New `sqlite::agents` mirrors the PG free-fn surface —
  list / get (both with the `LEFT JOIN monitors … COUNT` for `monitor_count`),
  create (issues an `rmpa_<40>` token, stores only its SHA-256), update
  (empty-string location clears, `None` leaves), delete (one tx: drop the agent
  then `agent_id = NULL` its monitors, standing in for PG's `ON DELETE SET
  NULL`), `lookup` (token resolver), and `touch_seen`. `online` is derived from
  `last_seen_at` against `ONLINE_GRACE_SECONDS` at read time, as in PG. Closes
  the last tag-independent deferred `sqlite::monitors` item:
  `list_stale_agent_monitors` (agent-assigned + active + non-paused monitors
  whose newest heartbeat — or `updated_at` — predates `interval*2 + 30s`,
  paired with the agent name). +3 `#[sqlx::test]` (create/lookup/touch/count,
  update-clear/delete-unassigns, and the watchdog incl. heartbeat-clears-stale).
  **SQLite domains now 8: settings, orgs, users, sessions, monitors,
  heartbeats, tags, agents (21 tests).** Remaining deferred: `bulk_edit` /
  `bulk_edit_preview` and the heartbeat analytic aggregations. Off-by-default
  `sqlite` feature; PG build untouched.

---

## [0.156.6] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `tags` domain + monitor tag hydration / bulk flip.**
  New `migrations-sqlite/0005_tags.sql` (`tags` with per-org `(org_id, name)`
  unique index forked from PG `tags_org_name_uidx`, `monitor_tags`, and the
  `notification_tags` / `group_tags` join tables so `usage()` can COUNT
  channel/group attachments before those domains are forked). New
  `sqlite::tags` mirrors the full PG free-fn surface — list / get / create
  (unique-name → `Conflict`) / update / `usage` / delete / attach / detach /
  `list_for_monitor` / `hydrate_for_channels` / `hydrate_for_monitors` (the
  batch hydrators build a bound `IN (?,?,…)` list — sqlx 0.9 `SqlSafeStr` via
  `AssertSqlSafe`, placeholder count from the slice, values always bound).
  Closes two of the deferred `sqlite::monitors` items: read paths
  (`get` / `get_unscoped` / `list` / `list_all`) now hydrate `m.tags` (single
  fetch for one monitor, one batched round trip for lists), and
  `set_active_by_tag` (org-scoped bulk active/paused flip, idempotent via
  `active <> ?`). +2 `#[sqlx::test]` (CRUD + conflict; attach/hydrate/usage +
  monitor flip + detach). **SQLite domains now 7: settings, orgs, users,
  sessions, monitors, heartbeats, tags (18 tests).** Still deferred (need
  unbuilt tables): `list_stale_agent_monitors` (agents), `bulk_edit` /
  `bulk_edit_preview`, and the heartbeat analytic aggregations. Off-by-default
  `sqlite` feature; PG build untouched.

---

## [0.156.5] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `monitors` deferred-fn backfill (write/lifecycle
  surface).** Fills the heavier `sqlite::monitors` methods left out of 0.156.4:
  `update` (COALESCE patch over 21 fields + per-field clears for the double-Option
  group — `group_id` / `slo_target_pct` / `slo_window_days` / `agent_id` /
  `escalation_policy_id`, so an explicit `null` clears and a missing key leaves
  the column), `set_group`, the push/run lifecycle (`generate_push_token` →
  `regenerate_push_token` (push-kind + org gated) → `find_by_push_token`,
  `mark_run_started` → `push_state` → `close_run` (returns the prior run start) →
  `bump_push_at` / `fetch_last_push_at`), `set_cert_info`, the SLO trio
  (`slo_state` / `mark_slo_breached` / `clear_slo_breached`), `list_for_agent`,
  and `public_fields_batch` (per-id loop — sqlx 0.9 `SqlSafeStr` rejects the
  dynamic `IN (...)`). 2 more `#[sqlx::test]` cover the update double-Option
  clear/set, group + agent reassign, push/run round-trip, cert, and SLO breach
  lifecycle. **SQLite monitors now: 16 tests.** Still DEFERRED (need unbuilt
  tables): `list_stale_agent_monitors` (agents), `set_active_by_tag` /
  `bulk_edit` / tag hydration (tags / monitor_tags), and the heartbeat analytic
  aggregations. Off-by-default `sqlite` feature; PG build untouched.

---

## [0.156.4] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `monitors` + `heartbeats` (core monitoring entity).**
  `migrations-sqlite/0004_monitors.sql` (the wide monitors table + heartbeats
  time-series, forked from PG: 40+ cols, `monitor_kind`/`monitor_status` enums →
  TEXT, `int[] accepted_statuses` → JSON TEXT, jsonb → TEXT, `numeric
  slo_target_pct` → REAL, timestamps → INTEGER `unixepoch`). `sqlite::monitors`
  core CRUD — create / get / get_unscoped / list / list_all / delete /
  set_active / set_status (wide row read via the `sqlx::Row` get-by-name API;
  enums round-tripped through serde, not a 40-arm match). `sqlite::heartbeats` —
  `insert_many` (per-row in a tx; no UNNEST), `recent_for_monitor`, and
  `uptime_pct` (COUNT-ratio). 2 `#[sqlx::test]` pass incl cross-org isolation +
  uptime math + `(monitor_id, ts)` idempotency. DEFERRED (heavier /
  dialect-divergent): monitor update/bulk_edit/push/SLO/cert/agent/tag-ops + tag
  hydration, and the heartbeat analytic aggregations (percentile/window/
  error-budget — need app-side compute). Off-by-default `sqlite` feature; PG
  build untouched. **6 SQLite domains now: settings, orgs, users, sessions,
  monitors, heartbeats (14 tests).**

---

## [0.156.3] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `sessions` domain.** `sqlite::sessions` with all 9
  `StoreSessions`-equivalent fns (create [random v4 id, INTEGER unix expiry] /
  get [expiry-filtered] / set_active_org [owner-scoped] / delete /
  delete_for_user / list_for_user / delete_one_for_user / delete_others /
  cleanup_expired) + `migrations-sqlite/0003_sessions.sql`. This also **closes the
  last users parity gap**: `users::set_admin` / `set_role` / `disable_totp` now
  revoke the user's sessions (via `sessions::delete_for_user`) so a privilege or
  2FA downgrade takes effect immediately — matching Postgres. 2 `#[sqlx::test]`
  pass (create/lookup/expiry/active-org/revoke + the cross-domain
  role-change-revokes-sessions). A SQLite Rampart can now do the full
  login/session/org flow. Off-by-default `sqlite` feature; PG build untouched.

---

## [0.156.2] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `users` domain.** `sqlite::users` with all 20
  `StoreUsers`-equivalent fns — count / create (seeds the Default-org membership
  atomically) / get / get_by_email (`UserWithHash`) / by_email / TOTP
  set-secret+enable+disable / mark_login / the durable TOTP-lockout
  (record_failure + locked-until + reset, `unixepoch()`-based) / list / set_admin
  + set_role (mirror the role onto the Default-org membership in-tx, like PG) /
  delete / GDPR anonymize / prefs / set_password. 4 `#[sqlx::test]` SQLite tests
  pass (membership seeding, case-insensitive email via `COLLATE NOCASE`,
  dup-email → `Conflict`, role-mirror, TOTP lockout lifecycle, anonymize). Parity
  gap noted: `set_admin`/`set_role`/`disable_totp` don't yet revoke sessions (the
  SQLite `sessions` domain isn't built — next). Off-by-default `sqlite` feature;
  PG build untouched.

---

## [0.156.1] — 2026-06-23

### Added
- **Multi-DB P1: SQLite `orgs` domain (identity/tenancy core).** Building out
  `SqliteStore` domain-by-domain on the P1-0 foundation. Restructured the SQLite
  backend into a `rampart_db::sqlite` module dir (shared conversions — `Role`↔TEXT,
  uuid↔TEXT, unix-seconds↔`OffsetDateTime`); added `migrations-sqlite/0002_identity.sql`
  (users / organizations / org_members, forked from the current PG schema —
  citext→`COLLATE NOCASE`, enum→CHECK'd TEXT, timestamptz→INTEGER `unixepoch()`,
  the slug regex→GLOB negated-class, + the seeded Default org) and `sqlite::orgs`
  with all 12 `StoreOrgs`-equivalent fns (create / get / by-slug / rename /
  orgs-for-user / member upsert+role+list+detailed+remove / admin-count / atomic
  create-with-owner). 6 `#[sqlx::test]` SQLite tests pass — including dup-slug →
  `Conflict` (sqlx's `is_unique_violation` works on SQLite) and the
  members-detailed JOIN. Still off-by-default `sqlite` feature; default PG build
  untouched. Next: the `users` domain, then `RAMPART_DB_URL=sqlite` wiring.

---

## [0.156.0] — 2026-06-23

### Added
- **Multi-DB P1-0: SQLite backend foundation (spike).** First step of the SQLite
  tier (single-binary / homelab) now that the P0 `Store` seam is complete.
  Behind an off-by-default `sqlite` cargo feature so the Postgres reference build
  + its `.sqlx` cache are completely untouched. Lands: the `sqlx` SQLite driver,
  a parallel `migrations-sqlite/` set (the `settings` table to start), a
  `rampart_db::sqlite` module, and a `#[sqlx::test]`-backed test proving the
  SQLite per-test-DB fixture framework (the plan's #1 risk) works end-to-end.
  Surfaced + recorded the core mechanical blocker for the full backend — a crate
  can't hold both PG and SQLite `query!` macros validated against one DB, and the
  offline `.sqlx` cache can't hold both without each `sqlx prepare` run pruning
  the other — so the SQLite layer uses runtime-checked `sqlx::query` (covered by
  the `#[sqlx::test]` suite) until the PG query layer is `cfg`-gated in a later
  slice. See `docs/design/MULTI_DB.md` (P1 plan). No effect on the default build.

---

## [0.155.23] — 2026-06-22

### Changed
- **Accessibility: focus-trap + ARIA on the secret-reveal + create modals**
  (Ingest Keys, API Keys, Agents). All six modal dialogs across those views now
  use the shared `useFocusTrap` hook — Tab cycles within the dialog, focus moves
  in on open and restores to the opener on close, and Escape closes — and carry
  `role="dialog"` / `aria-modal="true"` / `aria-label` / `tabIndex={-1}`. Matches
  the dialog primitive the monitor-edit + dashboard modals already use, so the
  show-once token reveals are keyboard- and screen-reader-accessible. (six-persona
  audit #21, modal half.)

---

## [0.155.22] — 2026-06-22

### Added
- **Delivery-log filter controls (dashboard).** The Delivery Log view gains
  three dropdowns — outcome (delivered/failed), channel kind, and monitor — that
  drive the server-side filters added in 0.155.21. Changing a filter resets to
  the newest page; a "Clear filters" button appears when any is active. Channel
  options come from the rows on screen (plus the active selection), monitors from
  the existing monitor list. Completes six-persona audit #22.

---

## [0.155.21] — 2026-06-22

### Added
- **Delivery-log server-side filters.** `GET /v1/delivery-log` now accepts `ok`
  (true/false outcome), `monitor_id` (UUID), and `channel_kind` query params, on
  top of the existing `before`/`limit` keyset pagination — so an operator can
  pull "all failed Slack deliveries for monitor X" directly instead of scanning
  pages client-side. Filters compose and are org-scoped + null-guarded in one
  query. (six-persona audit #22, backend half; the dashboard filter controls
  are a follow-up — the JSON list API honours the params today.)

---

## [0.155.20] — 2026-06-22

### Added
- **Continuous audit-chain integrity monitoring (SIEM/compliance).** The
  scheduler now re-verifies the tamper-evident audit hash chain on a leader-only
  slow-tick, throttled to ~hourly via a `settings` watermark (so the full
  re-walk doesn't run every 30s). A broken chain — an `audit_log` row edited,
  deleted, or reordered — is surfaced two ways: a high-severity `error!` log
  (captured by Rampart's own self-telemetry, so operators can alert on it with a
  telemetry/detection rule) **and** an `audit.chain_verify_failed` audit event
  (the forward append continues the chain). The watermark records the last
  verify time + outcome. Previously chain verification was only manual (the
  admin `/v1/audit-log/verify` endpoint); tampering is now caught proactively.
  (six-persona audit #6.)

---

## [0.155.19] — 2026-06-22

### Changed
- **Frontend: stop refetching `/v1/auth/me` on every view navigation.** The
  mount-time auth check had `route.view` in its effect deps, so every click
  between views (dashboard → logs → traces → …) fired a redundant `/me`
  round-trip — the session and per-org role don't change as you navigate. Now a
  true one-shot on mount. The cases that DO change auth already drive the
  refresh directly (Login's `onAuthed`, inline logout state-clear, full reload
  on org switch); session expiry is still caught lazily (any view's API call
  401s and the request layer redirects to `#/login`). Snappier navigation, less
  server load. (six-persona audit #27.)

---

## [0.155.18] — 2026-06-22

### Added
- **Access-review report (compliance-evidence epic, slice 2).** New admin-gated
  endpoints `GET /v1/compliance/access-review` (JSON) and
  `/v1/compliance/access-review.csv` (auditor evidence download) return a
  point-in-time snapshot of every `(org, member)` access grant — org, email,
  role, member-since, last login, and MFA status — joined across
  `org_members` / `users` / `organizations` in one read. This is the standing
  user-access table a SOC 2 CC6 (logical access) review asks for; it complements
  slice 1 (GDPR export/erasure) and the audit log (which records access-change
  *events*, not current *state*). Pulling a report is itself audited
  (`compliance.access_review`). Read-only, no schema change; new
  `rampart_db::access_review` module + `StoreCompliance` seam method. CSV reuses
  the shared escaper.

---

## [0.155.17] — 2026-06-22

### Added
- **Syslog + JSON-lines log ingest (SIEM epic).** Two new public ingest
  endpoints land external logs straight into the existing log tier (and, for
  free, the detection engine — which matches the same fields):
  - `POST /syslog` — `text/plain`, one or more newline-framed **RFC 5424** or
    **RFC 3164** lines (what rsyslog / syslog-ng emit). PRI severity maps to the
    OTLP severity scale; hostname / appname / procid / structured-data are
    preserved under `attributes`.
  - `POST /syslog/json` — **NDJSON**, one JSON log object per line; recognises
    the common field aliases (`level`/`severity`, `message`/`msg`/`body`,
    `service`/`service.name`, `timestamp`/`ts`) and keeps the full object in
    `attributes`.
  Auth + org resolution reuse the shared ingest-credential path (Bearer /
  `X-Rampart-Token` / `?k=`, Default org when token-less), respect the
  configured log head-sampling, and inflate gzip bodies — same surface as
  `/otlp`. Malformed lines are skipped, never fatal to the batch. Parsing lives
  in `rampart_core::syslog` (RFC5424 / RFC3164 / NDJSON → `ParsedLog`). No
  schema change — reuses the `logs` table + `insert_logs`. Returns
  `{ "accepted": N }`. Previously Rampart only *exported* syslog (to an upstream
  SIEM); it now ingests it too.

---

## [0.155.16] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to the `monitors` domain — the biggest /
  highest-churn domain — completing the multi-DB P0 seam (slice 13).** Added
  `StoreMonitors` (27 methods, `_monitor(s)`-suffixed) into the `Store`
  super-trait and migrated the **17** monitor free fns that have rampart-api
  call sites (create / get / get_unscoped / list / list_for_agent / update /
  delete / set_active / set_active_by_tag / set_group / set_status /
  regenerate_push_token / bulk_edit / bulk_edit_preview / find_by_push_token /
  mark_run_started / close_run) from `state.pool()` to `state.store()` across
  the monitors, monitor_groups, push, tags, notifications, monitor_templates,
  maintenance, escalations, agents, routing route files + external_ingest. The
  internal write transaction inside `bulk_edit` and the private generic
  `load_prior` stay encapsulated (object-safe — they're not in any public
  signature). The remaining 10 trait methods (`list_all`, `set_cert_info`,
  `slo_state`, etc.) are added for a 1:1 mirror but their only callers are the
  not-seam-aware scheduler / status_pages / seed / `bin/import`, which keep the
  free fns. Zero behavior change; `Store` still object-safe. **Every clean
  rampart-db domain is now behind the `Store` seam** — the P0 seam extraction is
  complete; next is the per-driver backends (SQLite first).

---

## [0.155.15] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to the `audit` domain (multi-DB P0
  slice 12).** Added `StoreAudit` (7 methods — `record_audit`,
  `verify_audit_chain`, `audit_security_insights`, `list_audit_entries`,
  `fetch_audit_since`, `export_audit_batch`, `set_audit_chain_watermark`) into
  the `Store` super-trait. To keep the seam surface backend-neutral, `NewEntry`
  now carries `Option<std::net::IpAddr>` instead of the PG-specific
  `sqlx::IpNetwork`; `audit::insert` converts to `IpNetwork` once and uses that
  same value for both the column bind and the hash input, so the tamper-evident
  chain stays **byte-identical** to pre-refactor rows. The rampart-api audit
  wrapper (`record` / `record_anon` / `emit`) now threads `&Arc<dyn Store>` and
  routes writes through `store.record_audit` — **92** call sites across 23 route
  files migrated from `s.pool()` to `s.store()`; `client_ip()` returns
  `IpAddr`. The 5 audit read routes (insights / verify / list / list_csv /
  export_csv stream) go through the store. In-crate / non-seam-aware callers
  (`prune.rs` watermark, notifier `siem.rs` fetch_since, the chain tests) keep
  the free fns. Zero behavior change; `Store` still object-safe. Only the
  `monitors` special case remains unseamed.

---

## [0.155.14] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to the `orgs` domain (multi-DB P0
  slice 11).** Added `StoreOrgs` (12 methods — create / get / by-slug / rename /
  orgs-for-user / member upsert+role+remove / member listing / admin count /
  atomic create-with-owner) into the `Store` super-trait and migrated **20**
  rampart-api call sites across the org-management routes, the org-context auth
  middleware, `/v1/auth/me`, OIDC org-claim mapping, and the GDPR export. The
  per-org RBAC helpers (`require_org_role` / `last_admin_demotion`) now take
  `&dyn Store` instead of `&DbPool`. The one object-safety special case —
  `orgs::upsert_member`, generic over `sqlx::PgExecutor` for tx-atomic callers —
  stays a free fn; the seam exposes a pool-scoped `upsert_org_member` alongside
  it. Zero behavior change — verified by orgs_api 7, rbac 4, auth 11,
  multitenancy_isolation 11, and the rampart-db orgs unit tests (7). `Store`
  still object-safe. Only the audit (`IpNetwork`→`IpAddr`) + monitors special
  cases remain unseamed.

---

## [0.155.13] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to the `webpush` domain (multi-DB P0
  slice 10).** Added `StoreWebpush` (6 methods — subscription list / upsert /
  delete-by-endpoint / delete plus VAPID-key read / write) into the `Store`
  super-trait. The shared-VAPID get-or-create was the last `impl FnOnce`
  closure blocking object-safety: refactored `webpush.rs` into two object-safe
  primitives (`get_vapid` / `set_vapid`), keeping `get_or_create_vapid` as a
  thin free fn for the not-yet-seamed notifier crate. The rampart-api routes
  (`/v1/webpush/vapid-key` + subscribe/unsubscribe) now compose the get-or-
  create from the two store primitives, routing entirely through `state.store()`.
  Zero behavior change; `Store` still object-safe (compile-time assertion +
  clippy `--all-targets` green). Only the audit / orgs / monitors object-safety
  special cases remain unseamed.

---

## [0.155.12] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to the auth-critical `users` domain
  (multi-DB P0 slice 9).** Added `StoreUsers` (20 methods, `_user(s)`-suffixed,
  delegating to the existing free fns) into the `Store` super-trait, and
  migrated **41** rampart-api call sites across the auth surface (register /
  login / me / logout, RBAC + session middleware, TOTP, OIDC, user management,
  prefs, GDPR) to `state.store()`. Bare-pool callers in the reset-password CLI
  + seed keep the free fns. Zero behavior change — verified by the full auth
  surface: auth 11, rbac 4, gdpr 2, orgs_api 7, multitenancy_isolation 11,
  rampart-db lib 32 all green. Only `webpush` + the audit/orgs/monitors
  object-safety special cases remain unseamed.

---

## [0.155.11] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to 7 more domains (multi-DB P0 slice 8).**
  Added `StoreIncidentTemplates`, `StoreMonitorPresets`, `StoreMonitorTemplates`,
  `StoreDeliveryLog`, `StoreAgents`, `StoreMetricSamples`, `StoreSourceMaps`
  sub-traits (34 methods, per-domain-suffixed, delegating to existing free fns)
  into the `Store` super-trait, and migrated **32** rampart-api call sites to
  `state.store()`. **40 of ~40 domains** now reach the DB through the object-safe
  `&dyn Store` seam. `webpush` deferred (its `get_or_create_vapid` takes a
  generic closure → not object-safe; joins audit/orgs/monitors/users as the
  remaining object-safety special cases). Zero behavior change.

---

## [0.155.10] — 2026-06-22

### Added
- **GDPR data export + right-to-erasure (compliance epic, slice 1).** Two new
  admin-only, audited endpoints on the user resource:
  - `GET /v1/users/{id}/export` — aggregates a user's personal data (profile,
    UI preferences, active sessions, org memberships) into one JSON document
    for a data-subject access request.
  - `POST /v1/users/{id}/erase` — right-to-erasure by **anonymizing in place**
    (email tombstoned, name + 2FA cleared, password made non-verifiable) and
    revoking all sessions + recovery codes. The row is kept as an anonymized
    tombstone so the append-only tamper-evident audit chain and FK references
    stay intact (security-log legal-retention exception) — a hard delete is
    impossible anyway because `audit_log.actor_user_id` RESTRICTs. Cannot erase
    your own account; the erasure action is itself audited.
  Integration-tested (export→erase→login-fails + self-erase guard). First slice
  of the SOC2/ISO/GDPR compliance-evidence epic.

---

## [0.155.9] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to 9 more domains (multi-DB P0 slice 7,
  big batch).** Added `StoreNotifications`, `StoreSettings`, `StoreLogs`,
  `StoreTraces`, `StoreRum`, `StoreProfiles`, `StoreMetrics`,
  `StoreErrorTracking`, `StoreScheduledReports` sub-traits (84 methods,
  per-domain-suffixed, delegating to the existing free fns) into the `Store`
  super-trait, and migrated **137** rampart-api route/handler call sites from
  `rampart_db::X::fn(state.pool(), …)` to `state.store().method(…)`.
  **33 of ~40 domains** now reach the DB through the object-safe `&dyn Store`
  seam. Zero behavior change (same SQL, same pool, same RLS hooks; bare-pool
  callers in scheduler/notifier/seed keep the free fns). Multi-DB groundwork.

---

## [0.155.8] — 2026-06-22

### Changed
- **Navigation menu is now an obvious, labelled launcher on every page.** The
  global nav drawer's launcher was an unlabelled ☰ icon in the corner, easy to
  miss — so from a sub-view users went back to the dashboard to navigate. It is
  now a clearly-labelled **"☰ Menu"** pill, present on every authenticated view,
  opening the same role-filtered drawer.

### Internal
- **e2e: cross-browser navigation robustness (full 5-browser matrix green).**
  Firefox + WebKit abort `page.goto` mid-navigation (`NS_BINDING_ABORTED`,
  "interrupted by another navigation", transient "WebKit encountered an internal
  error") when the SPA redirects while a goto is settling — chromium tolerates
  it. Routed all e2e navigations through a `robustGoto` helper that retries on
  those engine-level aborts. chromium / firefox / webkit all 49/49 locally
  (chrome + msedge ride the chromium engine). Test-harness only.

---

## [0.155.7] — 2026-06-22

### Fixed
- **Public status pages served stale data for up to 10s after a change.** The
  per-slug public-view cache (10s TTL) was never invalidated on writes, so a
  resolved incident, posted incident update, or status-page/section edit kept
  showing the old projection until the TTL lapsed. Every incident + status-page
  + section mutation (management API *and* the webhook/vendor-ingest incident
  paths) now drops the cache so the next public read re-projects fresh.
- **`/v1/auth/me` was rate-limited, bouncing users to login during normal use.**
  The whole `/auth` router (including the cheap, SPA-polled `me` + `logout`) sat
  under the per-IP auth brute-force limiter (10 burst), so clicking through views
  quickly — or any page that fetches `me` a few times — could exhaust the bucket
  and 429, which the SPA treats as logged-out. The limiter now scopes to the
  brute-forceable surface only (`login`, `register`, 2FA-verify, OIDC); `me` +
  `logout` ride free. Burst protection on login/register/2FA is unchanged.
- **Unknown custom-domain lookups returned `200 null` instead of `404`.**
  `GET /v1/public/status-pages/by-domain/{host}` for an unconfigured host now
  returns `404` (matching the documented contract + the frontend host probe).

### Internal
- **e2e suite green end-to-end (49/49 chromium).** Hardened the Playwright
  harness: deterministic first-run-vs-login detection via the API (no more
  racing the `/me` needs-setup probe), a short timeout on the best-effort
  monitor-detail "remove channel" click so the API fallback runs, and a
  re-login step in the TOTP spec after disabling 2FA (which correctly revokes
  all sessions). No product behavior change from these.

---

## [0.155.6] — 2026-06-22

### Fixed
- **Auth: signup/login → dashboard navigation race.** After a successful
  first-run signup (or login), the SPA navigated to `#/` before its auth state
  refreshed, so the route gate saw a stale `user: null` and bounced the
  just-authenticated user back to `#/login` — where a one-shot `needs_setup`
  check kept them stuck despite a valid session. `Login.jsx` now awaits a
  shared `refreshAuth()` (exposed by `App.jsx`) before navigating, so the gate
  sees the live session on the next render. Surfaced by the e2e suite; affects
  the login→dashboard transition under realistic timing.

### Internal
- **e2e: per-test client-IP isolation for the auth rate-limiter.** The
  auth-surface limiter (10-burst per client IP, added with the trusted-peer IP
  resolution) pooled the whole Playwright suite's logins into one loopback
  bucket, so the suite starved itself with 429s. The harness now stamps a
  unique `X-Forwarded-For` per browser/API context (`e2e/fixtures.js`) and the
  test webServer sets `RAMPART_TRUSTED_PROXIES` to the loopback peer so the
  server honours it. Test-environment only — production still keys on the real
  TCP peer, default burst unchanged.

---

## [0.155.5] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to 6 more (heavier) domains (multi-DB P0
  slice 6, big batch).** Added `StoreStatusPages`, `StoreIncidents`,
  `StoreRouting`, `StoreSubscribers`, `StoreDetection`, `StoreSessions`
  sub-traits (78 methods, per-domain-suffixed, delegating to the existing free
  fns) into the `Store` super-trait, and migrated **103** rampart-api
  route/handler call sites — including the public status-page routes and the
  session/auth path — from `rampart_db::X::fn(state.pool(), …)` to
  `state.store().method(…)`. **24 of ~40 domains** now reach the database
  through the object-safe `&dyn Store` seam. Zero behavior change (same SQL,
  same pool, same RLS hooks; bare-pool callers in the scheduler/seed keep the
  free fns). Part of the multi-DB backing-store groundwork.

### Fixed
- **CI: backend `clippy + fmt` job restored to green.** Stable `rustfmt`
  advanced to 1.9.0 (2026-05-25) and changed line-wrapping rules, so the
  `cargo fmt --all -- --check` step had begun failing on `main`. Reformatted
  the workspace (pure formatting, zero behavior change) and added `fmt --check`
  to the local release gate so it can't silently drift again.

---

## [0.155.4] — 2026-06-22

### Changed
- **Internal: `Store` seam extended to 10 more domains (multi-DB P0 slice 5, bigger
  batch).** Added `StoreEscalations`, `StoreMaintenance`, `StoreIngestTokens`,
  `StoreTags`, `StoreTemplates`, `StoreTelemetryRules`, `StoreMetricRules`,
  `StoreMonitorGroups`, `StoreSilences`, `StoreOidcState` sub-traits (91 methods,
  per-domain-suffixed, delegating to the existing free fns) into the `Store`
  super-trait, and migrated **70** rampart-api route/handler call sites from
  `rampart_db::X::fn(state.pool(), …)` to `state.store().…`. **Zero behavior
  change** (same SQL, same pool + RLS hooks); scheduler/notifier/seed bare-pool
  callers stay on the free fns and coexist. No SQL/`.sqlx`/migration change.
  **18 of ~40 domains now flow through the object-safe seam.**
  `monitors`/`audit`/`orgs::upsert_member` still pending object-safety cleanups.
  (`docs/design/MULTI_DB.md`.)

---

## [0.155.3] — 2026-06-22

### Changed
- **Internal: extended the `Store` seam to 4 more domains (multi-DB P0 slice 4).**
  Added `StoreProxies`, `StoreOnCall`, `StoreRecoveryCodes`, `StoreApiKeys`
  sub-traits (per-domain-suffixed methods delegating to the existing free fns)
  to the `Store` super-trait, and migrated ~18 rampart-api route/handler call
  sites — including the `lookup_api_key` bearer-auth hot path — from
  `rampart_db::X::fn(state.pool(), …)` to `state.store().…`. **Zero behavior
  change** (same SQL, same pool + RLS hooks); bare-pool callers in
  scheduler/notifier/seed stay on the free fns and coexist. No SQL/`.sqlx`/
  migration change. Eight of the ~40 domains now flow through the object-safe
  seam; `monitors`/`audit`/`orgs::upsert_member` still pending their
  object-safety cleanups. (`docs/design/MULTI_DB.md`.)

---

## [0.155.2] — 2026-06-22

### Changed
- **Internal: the `Store` seam is now load-bearing (multi-DB P0 slice 3).**
  Extended `rampart-db::store` with three more object-safe domain sub-traits —
  `StoreDeployMarkers`, `StoreIngestKeys`, `StoreSlos` (per-domain-suffixed
  methods, e.g. `create_deploy_marker`/`create_ingest_key`/`create_slo`, each
  delegating to the existing free fn) — and added them to the `Store`
  super-trait. Migrated the 10 rampart-api route call sites for those domains
  from `rampart_db::X::fn(state.pool(), …)` to `state.store().…`, so the seam is
  actually exercised (slice 2 added it unused). **Zero behavior change:** the
  trait methods run the same SQL on the same pool (same RLS hooks); bare-pool
  callers in the scheduler/notifier/seed stay on the free fns and coexist. No
  SQL/`.sqlx`/migration change. `monitors`/`audit`/`orgs::upsert_member` stay
  out (object-safety cleanups deferred to a later slice). (`docs/design/MULTI_DB.md`.)

---

## [0.155.1] — 2026-06-22

### Changed
- **Internal: introduced the object-safe `Store` seam (multi-DB P0 slice 2).** A
  new `rampart-db::store` module defines a `Store` super-trait composed of an
  object-safe `StoreHeartbeats` domain sub-trait (one method per public
  `heartbeats` fn, with the `pool` arg replaced by `&self`), a single Postgres
  impl `PgStore` that delegates each method straight to the existing
  `heartbeats::*` free functions, and a compile-time object-safety guard
  (`const _: fn(&dyn Store) = …`). `AppState` gains an additive
  `Arc<dyn Store>` field + `store()` accessor. This proves the `&dyn Store`
  super-trait shape is object-safe and the wiring compiles — de-risking the full
  ~40-trait extraction — with **zero behavior change**: no SQL, no `.sqlx`
  change, no caller migrated (`store()` has no callers yet), every existing
  `.pool()` path untouched. (`docs/design/MULTI_DB.md`.)

---

## [0.155.0] — 2026-06-22

### Fixed
- **RUM pages were indistinguishable across apps when "All apps" was selected.**
  The per-URL pages rollup grouped by `url` only, so the same path on two sites
  collapsed into one row with no way to tell which app it belonged to, and the
  drill-down mixed every app's samples. The rollup now groups by `(app, url)`
  and returns the app; the pages table shows an app badge per row when "All
  apps" is selected, row identity is `(app, url)` so same-path-different-app
  rows are independent, and the per-page drill-down scopes to that row's own
  app. (Reported.)

---

## [0.154.2] — 2026-06-22

### Fixed
- **Monitor → Config tab white-paged for any monitor with tags attached.** In
  `TagsCard`, the attached-tags `.map(t => …)` loop variable shadowed the
  imported i18n function `t`, so the `t('common.detach')` call inside rendered a
  *tag object* as a function → `TypeError` → the whole Config tab crashed to a
  blank page. Renamed the loop variable to `tag` (same `t()`-shadow class as the
  earlier SLO-edit crash). Verified no other `.map(t => …)` body calls `t()`.

---

## [0.154.1] — 2026-06-22

### Changed
- **Internal: removed the last cross-function transaction threading in
  rampart-db** (multi-DB groundwork, no behavior change). `orgs::upsert_member`
  is now generic over `sqlx::PgExecutor` and the redundant `upsert_member_tx`
  (which took a `&mut Transaction` — the lone function exposing a concrete
  Postgres transaction across a boundary) is deleted; its 4 callers
  (`users::create`, `set_admin`, `set_role`, `orgs::create_with_owner`) now pass
  `&mut *tx` to the unified helper. Atomicity is byte-identical (same statements,
  same begin/commit points, verified by the existing org-membership
  `#[sqlx::test]`s). This removes the object-safety wall blocking a future
  driver-agnostic `Store` trait — the first slice of the multi-DB P0 spike
  (`docs/design/MULTI_DB.md`). No SQL, schema, or `.sqlx` change.

---

## [0.154.0] — 2026-06-22

### Security
- **Client IP for rate-limiting + audit now derives from the real TCP peer, not
  the spoofable `X-Forwarded-For`.** Both the per-IP rate limiters (auth
  brute-force + ingest) and the audit/session source IP read the leftmost
  `X-Forwarded-For`/`X-Real-IP` — fully forgeable by any direct client, so an
  attacker could rotate XFF to evade the auth brute-force cap, burn a victim's
  bucket, or frame an arbitrary source IP in the SIEM-exported audit log. The
  client IP is now resolved from the axum `ConnectInfo` TCP peer, honoring
  `X-Forwarded-For` **only** when the peer is a configured trusted proxy
  (new `RAMPART_TRUSTED_PROXIES` allow-list of IPs/CIDRs), taking the
  rightmost non-trusted XFF entry across all header lines. A single outermost
  middleware resolves the IP once; rate-limit, audit, and session-create all
  consume the trusted value. IPs are canonicalized (IPv4-mapped-IPv6 safe). As
  a bonus, session rows — which previously recorded **no** client IP — now
  record the resolved IP. See [`docs/SETUP.md`](docs/SETUP.md).

  **BREAKING for reverse-proxied deployments:** the default (unset
  `RAMPART_TRUSTED_PROXIES`) ignores `X-Forwarded-For` and uses the direct TCP
  peer — secure on a fresh/direct install, but a deployment behind a reverse
  proxy/LB **must set `RAMPART_TRUSTED_PROXIES` to the proxy's exact IP(s)** or
  every request's per-IP rate-limit bucket + audit source IP collapses to the
  proxy IP (shared-bucket auth false-lockout risk). Set it to the *specific*
  proxy IP(s) — never a broad internal range (e.g. `10.0.0.0/8`) that also
  contains untrusted hosts, since any host inside a trusted CIDR can forge the
  client IP. A loud startup warning fires when the var is unset and the bind
  address is non-loopback. (Six-persona audit / track-4.)

---

## [0.153.1] — 2026-06-22

### Security
- **Detection windows now key off server `received_at`, not the client event
  timestamp.** The log-detection engine (`detection.rs`) and the `log_volume`
  telemetry rule windowed on `logs.ts` — the client-supplied OTLP
  `timeUnixNano`. An attacker who backdated `time_unix_nano` landed outside the
  `(last_checked_at, now]` window and **silently evaded detection**; benign
  client clock-lag also dropped legitimate events. All log-detection time
  windows (and their sample `ORDER BY`) now filter on the server-stamped
  `received_at`. Migration `0120` adds a `logs(org_id, received_at DESC)`
  composite index so org-scoped detection seeks its tenant slice (mirrors the
  spans/RUM/profiles composites). `trace_latency`/`trace_error_rate`/
  `profile_samples`/`rum_lcp_p75` already windowed on `received_at`; `error_rate`
  needed no change (`error_events.ts` is already server-stamped). (Six-persona
  audit #11.)

---

## [0.153.0] — 2026-06-22

### Added
- **OIDC id_token signature + claims validation (JWKS).** SSO previously
  established identity from the unauthenticated `userinfo` response and never
  validated an id_token. The callback now requires an id_token (scope `openid`),
  verifies its signature against the IdP's JWKS (matched by `kid`, fetched via
  the SSRF-guarded client, cached 1h with throttled rotation-refetch), pins
  validation to the token's single asymmetric algorithm (allow-list rejects
  `alg=none`/HMAC and a symmetric `oct` JWK — the classic alg-confusion attacks),
  and enforces `iss`/`aud`/`exp`/`nbf` + a one-time `nonce` (constant-time
  compared, bound to the consumed login state). Identity is taken from the
  verified id_token with userinfo as a fallback; the `email_verified` gate and
  org-claim mapping are unchanged. (Six-persona audit #13.)

### Changed / Security
- **OIDC login state is now stored in Postgres (HA-correct).** The
  `state → PKCE-verifier (+ nonce)` map was a process-local `Mutex<HashMap>`, so
  under HA the `/login` and `/callback` could hit different replicas → 401, and
  it was wiped on every restart. Migration `0119` adds an `oidc_login_state`
  table (pre-auth, not org-scoped); consume is an atomic `DELETE … RETURNING`
  (one-time-use, replay-safe across replicas), rows carry a 10-min TTL and are
  reaped by the existing leader-gated prune sweep.
- **OIDC outbound is SSRF-guarded.** Discovery/token/userinfo/JWKS calls used a
  bare `reqwest::Client` with no guard or timeout; they now use the guarded
  client (vets every dialed IP incl. redirects) + a literal-IP `guard_url`
  preflight + 30s/10s timeouts. A new narrow rampart-ssrf allow-list lets a
  private self-hosted IdP (Keycloak/Authentik on 10.x/192.168.x) resolve under
  `RAMPART_SSRF_BLOCK_PRIVATE` via the configured issuer host **only** —
  metadata/loopback/link-local stay blocked unconditionally. `/auth/oidc` is now
  under the per-IP auth rate-limiter (bounds the unauthenticated state INSERT).
  See [`docs/SETUP.md`](docs/SETUP.md).

---

## [0.152.6] — 2026-06-22

### Security
- **Ingest credentials are now hashed at rest (SHA-256), Phase A.** `ingest_keys`
  (per-org OTLP/Prom/RUM/profiles keys) and `ingest_tokens` (per-status-page
  webhook tokens) were stored verbatim and resolved by `WHERE token = $1`,
  unlike their already-hashed peers `api_keys.key_hash` / `agents.token_hash`.
  Migration `0118` adds a `token_hash` column, backfills it from the existing
  plaintext (pgcrypto `digest()`, byte-identical to the Rust `sha256_hex`), and
  builds a UNIQUE index; the app now mints with the hash and resolves by it.
  **Non-breaking:** the credential value clients present is unchanged, every
  already-minted key/token keeps working (backfilled), and the plaintext column
  is intentionally KEPT this release so a rollback stays safe — lookups use a
  `token_hash OR token` fallback so a key minted by an old node mid-rolling-deploy
  still resolves. A follow-up migration (Phase D) drops the plaintext column
  (the point at-rest exposure is fully eliminated) and makes webhook tokens
  show-once. (Six-persona audit #19.)
- **Sentry DSN-key check is now constant-time.** The error-ingest auth compared
  the presented key with `!=` (short-circuits on the first differing byte — a
  timing oracle); it now uses the shared constant-time comparison. (#19.)

---

## [0.152.5] — 2026-06-21

### Security
- **RDAP (and DoH) probes now dial through the SSRF guard.** The RDAP client
  followed `rdap.org`'s 302 to per-TLD registries with an unguarded HTTP client —
  and since the redirect target is server-controlled, a hostile/compromised RDAP
  endpoint could redirect a probe to `169.254.169.254` (cloud metadata) or an
  internal host. Both clients are now built via `guarded_client_builder()`, which
  vets each hop's dialed IP. (Bug-hunt round 2.)

### Fixed
- **Two more remote-triggerable probe panics (byte-slice on a char boundary).**
  The SSDP and WebSocket probes truncated an untrusted reply preview with a
  **byte** slice (`&s[..80]` / `&m[..120]`) after only a byte-length check; a
  multi-byte char (or the 3-byte U+FFFD lossy marker from an SSDP datagram)
  straddling the cut panicked the probe task. Both truncate by `chars()` now.
  (Bug-hunt round 2; same class as the Telegram/Twilio fix in 0.152.1.)
- **Cron parser could silently mis-schedule on a huge step.** `"59/200 * * * *"`
  computed `lo + step` in a `u8` that overflows; with release `overflow-checks`
  off it wrapped silently (wrong cron bits → silent mis-scheduling), and panicked
  in debug. Use `checked_add` and stop at the bound. (Bug-hunt round 2.)
- **Status-page builder: a failed section assignment no longer shows as saved.**
  The optimistic `assignSection` write wasn't rolled back on error (unlike the
  sibling reorder handler), so a failed save looked permanent. Revert the
  optimistic override in the catch. (Bug-hunt round 2.)

---

## [0.152.4] — 2026-06-21

### Security
- **Closed four cross-tenant IDORs in the management API.** A second audit pass
  found several resources whose handlers/queries were never org-scoped (the
  `org_id` columns existed and writes stamped them, but reads/mutations didn't
  filter), so any editor in org A could reach org B's data:
  - **Maintenance windows** — `list`/`get` returned every org's windows and
    `update`/`delete`/`set_active`/`attach`/`detach` acted on any window by id.
    All CRUD is now `org_id`-scoped; attach/detach org-gate both the window and
    the monitor (cross-org → 404).
  - **Status-page subscribers** — the admin `list`/`delete` weren't gated by the
    parent page's org (PII: subscriber emails). Both now resolve the page scoped
    to the caller's org first.
  - **Deploy markers** — `delete` (and the `list` chart query) weren't org-scoped;
    now `AND org_id = $N`, so cross-org delete/read 404s/returns nothing.
  - **Web-push subscribe** — bound a browser to any `notification_id`; now
    org-gates the target channel before the upsert.
  All fixes are migration-free (columns already exist) and reversible. (Bug-hunt
  round 2; same Phase-3 read-filtering pattern as the rest of the management API.)

---

## [0.152.3] — 2026-06-21

### Fixed
- **Scheduler `reconcile()` is now timeout-bounded like the other leader checks.**
  It runs first on every leading tick (`monitors::list_all` + per-monitor
  hydrate, no statement timeout); an unbounded slow reconcile under DB pressure
  stalled the whole loop — including the escalation paging that is deliberately
  ordered first. Wrapped in the same `timed()` guard as the periodic checks.
  (Bug hunt.)
- **OTLP nanosecond timestamps no longer wrap negative; enum fields are clamped.**
  Span start/end and log timestamps were `u64 as i64` casts — a value past
  `i64::MAX` wrapped negative, corrupting trace duration/ordering (and silently
  mis-stamping logs). They now use `i64::try_from(..)` with a 0 fallback. Span
  kind, status code, and log severity were `as i16` truncations of i32 enums (an
  out-of-spec `severityNumber` could alias onto a valid level); they are now
  clamped to their valid OTLP ranges. (Bug hunt.)

---

## [0.152.2] — 2026-06-21

### Security
- **Two ingest decompression bombs could OOM the process (unauthenticated on a
  default install).** The Prometheus `remote_write` path called snappy
  `decompress_vec`, which eagerly allocates the block header's *attacker-declared*
  decompressed length (up to ~4 GiB) before any validation — a tiny crafted POST
  to `/prom/write` triggered a multi-GiB allocation. The pprof ingest path
  inflated its inner gzip layer with an uncapped `read_to_end`, so a ~64 MiB
  gzip bomb expanded to tens of GiB. Both now enforce the same 64 MiB ceiling
  the gzip/deflate HTTP-layer decompressor already used (snappy: reject by
  `decompress_len` first; pprof: `take(MAX+1)` + length check). Since ingest is
  open by default when no telemetry token is configured, these were reachable
  pre-auth. (Bug hunt.)

---

## [0.152.1] — 2026-06-21

### Fixed
- **Telegram/Twilio alerts could panic and silently vanish on Unicode.** Both
  channels truncated the message with a **byte** slice (`combined[..1600]` /
  `[..4000]`); when a multi-byte char (emoji/CJK/accents — common in monitor
  names and probe error bodies) straddled the cut, Rust panicked. The panic was
  in the spawned dispatch task whose `JoinError` is swallowed, so the page was
  lost with no delivery-log row. Truncate by `chars()` instead (the pattern the
  other channels already use). (Bug hunt.)
- **A dead notification sink could hang dispatch forever and leak tasks.** The
  shared outbound HTTP client set no request/connect timeout (reqwest has none
  by default), so a sink that accepts the connection but never responds (tarpit,
  overloaded collector, malicious operator-set webhook) wedged the dispatch task
  indefinitely — and the fan-out then blocked awaiting it, leaking the task +
  socket across flapping monitors. Add a 30s request / 10s connect timeout so a
  dead target fails fast and the new transient-failure retry can kick in. (Bug
  hunt.)

---

## [0.152.0] — 2026-06-21

### Changed
- **Notification delivery now retries transient failures instead of dropping the
  page.** A failed channel dispatch previously logged the error, recorded the
  failure, and stopped — so a momentary network blip, a 429 rate-limit, or an
  upstream 5xx silently lost the alert. Dispatch is now retried up to 3 times
  with exponential backoff (0.5s, 1s), but **only** for failures where the prior
  attempt almost certainly didn't deliver: transport errors and retryable
  upstream statuses (408 / 429 / any 5xx). Permanent errors — bad config, an
  SSRF-blocked target, or a 4xx — are terminal (a retry would fail identically
  and just hammer the sink). Retrying is at-least-once, so a duplicate alert is
  possible but unlikely; for alerting a rare duplicate beats a dropped page.
  (Six-persona audit bigger-bet: alert delivery resilience.)

---

## [0.151.1] — 2026-06-21

### Security
- **Fonts are now self-hosted; the UI loads zero third-party assets.** Every
  view's CSS `@import`ed Google Fonts from `fonts.googleapis.com` (39 files),
  leaking each visitor's IP/User-Agent to Google on every page and breaking
  air-gapped installs. Inter + JetBrains Mono are now bundled via `@fontsource`
  and served same-origin, imported once at the app entry. The CSP drops
  `fonts.googleapis.com` from `style-src` and `fonts.gstatic.com` from
  `font-src` (both back to `'self'`) — combined with the prior client-side QR
  fix, the dashboard CSP now allow-lists no external origins for scripts,
  styles, or fonts. (Six-persona audit rank 1, fonts half.)

---

## [0.151.0] — 2026-06-21

### Added
- **SIEM export can emit CEF and LEEF, not just JSON.** The audit/findings
  forwarder previously shipped raw Rampart JSON, which Splunk/QRadar/ArcSight
  treat as an opaque blob. A new `format` setting (`json` | `cef` | `leef`)
  renders each event into ArcSight **Common Event Format** or IBM QRadar **Log
  Event Extended Format** so those SIEMs parse fields natively — severity-mapped,
  `src`-aliased, with proper header/extension escaping. Works over every sink
  (webhook posts CEF/LEEF as newline-delimited `text/plain`; syslog frames one
  record per line). The mapping is generic over the event shape, so new
  audit/finding fields appear automatically. Settings UI gains a format selector;
  the default stays `json`, so existing configs are unchanged. (Six-persona audit
  rank 12.) See [`docs/design/SIEM.md`](docs/design/SIEM.md).

---

## [0.150.7] — 2026-06-21

### Security
- **2FA QR no longer exfiltrates the TOTP secret to a third party.** The enroll
  panel rendered its QR by loading an `<img>` from `api.qrserver.com` with the
  full `otpauth://` URI — which embeds the base32 MFA seed — in the query string,
  handing every user's 2FA secret to an external service. The QR is now generated
  entirely client-side from the URI (zero-dependency `qrcode-generator` → inline
  SVG `data:` URI); the seed never leaves the browser, and enrollment now works
  offline / air-gapped. The CSP `img-src` allow-list drops `api.qrserver.com`
  accordingly. (Six-persona audit rank 1; the local-fonts half of that item —
  vendoring Google Fonts + tightening `style-src`/`font-src` — ships separately.)

### Changed
- **Retention prune: chunked deletes on the flat high-volume tiers.** The
  age-based DELETEs for logs, spans, metric_samples, RUM, profiles, audit_log and
  detection_findings ran as a single unbounded DELETE each, which on a large
  backlog took a long ACCESS-EXCLUSIVE lock and ballooned WAL (a contributor to
  the disk-pressure history). They now delete in 10k-row chunks via a shared
  helper until drained, bounding each statement's lock/WAL footprint. The
  heartbeat tier keeps its single rollup→delete transaction (its atomicity is
  load-bearing: a crash must never drop raw rows not yet rolled up). (six-persona
  audit rank 15.)

### Fixed
- **SLO error budget no longer burned by planned maintenance.** Both SLI
  computations (`heartbeats::current_slo_uptime_pct` and the SLO evaluator's
  `monitor_ratio`) divided up-count by `COUNT(*)` of *all* heartbeats, so a
  maintenance window counted as non-up and ate the error budget. Both now
  exclude `status = 'maintenance'` from numerator and denominator — maintenance
  is neither uptime nor downtime. (The general status-page `uptime_pct` is left
  unchanged. Rollup-stitching for windows beyond raw-heartbeat retention is a
  separate follow-up.) (six-persona audit rank 16.)

### Changed
- **Scheduler leader loop: timeout-bound each periodic check + advance
  escalations first.** The 8 leader-only checks ran back-to-back every 30s, so a
  single slow scan under DB pressure delayed everything after it — including
  escalation paging, which ran last. Each check now runs under a 25s timeout
  (overrun → skipped this tick, retried next) so one slow scan can't stall the
  loop, and `check_escalations` runs **first** so open episodes page on time
  regardless of the rule scans. (six-persona audit rank 18.)

### Added
- **Self-observability metrics on `/metrics`.** Operators can now alert on
  Rampart itself degrading: `rampart_notifier_events_dropped_total` (an alert/
  page the notifier shed because its channel was full/closed — previously only a
  log line) and DB-pool saturation gauges `rampart_db_pool_connections` +
  `rampart_db_pool_idle` (idle→0 means queries are queuing). (six-persona audit
  rank 14.)

---

## [0.150.2] — 2026-06-21

### Added
- **Audit-log every RBAC / org-membership change.** `org.create`, `org.rename`,
  member add / role-change / remove now write an `audit_log` record (actor, IP,
  target, before→after role) — the access-change events a SOC2 access review and
  most security audits require, previously unrecorded. A CI guard
  (`scripts/check-audit-coverage.sh`, wired into `ci.yml`) fails the build if a
  future org-mutation handler ships without its audit call. (six-persona audit
  rank 9.)

### Security
- **TOTP / recovery-code brute-force lockout.** The 2FA verify step re-issued a
  fresh challenge on every wrong code with no failure counter, so a caller past
  the password gate could grind the 6-digit TOTP (10^6 space) or the recovery
  codes — an MFA-bypass / account-takeover path. A durable per-account counter
  (migration `0117`: `users.totp_failed_attempts` + `totp_locked_until`) now
  locks the verify step after 5 consecutive failures for 15 minutes: while
  locked the server refuses immediately **and withholds a fresh challenge**, so
  the loop can't continue without a new (rate-limited) password round-trip. A
  successful verify clears the counter. Durable so a restart can't reset an
  attacker's count and the lockout holds across replicas. (six-persona audit
  rank 8.)

### Added
- **Retention for security detection findings.** `detection_findings` grew
  unbounded; it now has a `findings_days` retention tier (default 90 — the
  security-event record outlives the high-volume telemetry tiers but stays
  bounded) pruned each sweep alongside the other tiers.

### Changed
- **Per-task startup jitter in the probe scheduler.** Every probe task fired its
  first check immediately on start, so monitors sharing an interval ran in
  lockstep — a thundering herd on the DB and outbound probes every cycle, worst
  right after a boot/leadership-acquire. After the (still-immediate) first probe,
  each task now offsets its tick phase by a random fraction of its interval
  (capped at 30s), de-synchronizing steady-state load. The offset is cancellable
  so a paused/deleted monitor still tears down promptly. (six-persona audit ranks
  7 + 17.)

---

## [0.149.2] — 2026-06-20

### Security
- **Refuse to start under a low-entropy `RAMPART_SECRET_KEY`.** A placeholder key
  (all-zeros, a repeated byte — anything with `< 8` distinct bytes) decoded fine
  and silently encrypted every channel secret under a guessable key — *false*
  at-rest assurance, worse than plaintext. The server now bails at startup with a
  clear message (`openssl rand -hex 32`). A genuine random key (~30 distinct
  bytes) is unaffected.
- **SSRF-preflight the browser-probe target.** The `browser` monitor forwards
  `monitor.url` to the renderer, which fetches it — so a target like
  `http://169.254.169.254/…` was an SSRF the HTTP probe blocks but the browser
  probe didn't. The target is now vetted via the shared SSRF guard (honors
  `RAMPART_SSRF_BLOCK_PRIVATE`); the renderer connection stays unguarded
  (operator infra). (six-persona audit rank 20.)

---

## [0.149.1] — 2026-06-20

### Fixed
- **Audit-log tamper-evidence no longer false-positives after retention prune.**
  `verify_chain` started from `prev_hash = NULL`, so once retention deleted the
  oldest hashed rows it reported `ok:false` on every check (crying wolf, masking
  real tampering). The prune now persists a **chain watermark** — the id + hash
  of the newest hashed row it deletes — and `verify_chain` seeds the chain from
  it, so a legitimate head-truncation verifies while deleting a surviving row or
  editing any row still breaks the chain. The watermark is stored as a sealed
  setting, so with `RAMPART_SECRET_KEY` set a DB-level attacker can't forge it to
  mask a malicious head deletion. (Regression tests: head-truncation verifies;
  post-watermark deletion still detected.)

### Added
- **Multi-tenancy: RLS enforcement turned on (S7) — `ENABLE`, not `FORCE`.**
  Migration `0116` enables row-level security on exactly the 34 tables that
  carry the `org_isolation` policy (derived from `pg_policies`, no hand-listing).
  Because tables are `ENABLE`d but not `FORCE`d, the table **owner** (the
  `DATABASE_URL` role that ran the migrations) stays exempt by ownership, so:
  - `RAMPART_RLS` **off** (default) → every checkout is the owner → nothing
    enforced → byte-identical to before;
  - `RAMPART_RLS` **on** → tenant requests run as the non-owner `rampart_app`
    role and are **enforced** by the policies; system loops bind no org, stay
    the owner, and bypass for free.
  This **removes the `ALTER ROLE … BYPASSRLS` prerequisite** for the standard
  single-role deployment (the originally-planned `FORCE` would have required it
  and risked blacking out the background loops if forgotten). `RAMPART_RLS=0`
  reverts enforcement with no schema change. Multi-tenancy isolation is now
  defended at both the app layer (`WHERE org_id`) and the database layer (RLS).

### Added
- **Multi-tenancy: Postgres Row-Level Security scaffolding (defense-in-depth),
  behind `RAMPART_RLS` (opt-in, default OFF — flag-off is byte-identical, the
  full test suite passes unchanged).** This lands the safe, dormant slices
  (S1–S6); RLS is not yet enforced (`ENABLE`/`FORCE` is a separate held step).
  - `0114` — non-login `rampart_app` role + table/sequence grants (idempotent +
    login-role-agnostic so the test cluster is unaffected). No RLS enabled.
  - `0115` — `app_current_org()` helper (`NULLIF(current_setting(...,true),'')`
    so an unset GUC yields NULL, never a 500) + `org_isolation` policies on the
    30 tenant-root tables (+ 4 low-volume children via parent subquery);
    `heartbeats`/`error_events` deliberately excluded. **Policies ship DORMANT —
    no table has RLS enabled.**
  - When `RAMPART_RLS=1`, a tokio task-local + sqlx `before_acquire` hook binds
    the per-request org onto the connection (`SET ROLE rampart_app` +
    `set_config('app.current_org', …)`); system loops (scheduler/prune/notifier/
    self-metrics/migrate/import) bind no org and run as the BYPASSRLS owner. No
    change to the ~418 repository fns.
  - **Operator prerequisite (documented, for when RLS is force-enabled):** the
    `DATABASE_URL` login role must hold `BYPASSRLS`.

### Notes
- The actual enforcement flip (`ENABLE`/`FORCE ROW LEVEL SECURITY`) is held for a
  separate release after shadow-DB validation. See `docs/MULTITENANCY.md`.

---

## [0.147.0] — 2026-06-19

### Changed
- **Public status page: killed the 5N+ query amplification on the unauthenticated
  render path.** A page with N monitors previously issued ~1+6N DB queries per
  public hit (a per-monitor `get_unscoped` + four per-monitor heartbeat rollups
  in a loop) — query/DoS-amplification on an anonymous surface. Now:
  - **Set-based rollups:** four batch heartbeat fns (`uptime_pct_batch`,
    `avg_latency_ms_batch`, `daily_status_batch`, `monthly_uptime_batch`, all
    `WHERE monitor_id = ANY($1)` + `GROUP BY monitor_id`) plus a single
    `monitors` name/status fetch collapse the render to ~7 queries regardless of
    N. Output is byte-identical to the per-monitor path (covered by a new
    `public_view_batch_parity` test).
  - **Short-TTL per-slug cache (10s):** repeated public hits within the window
    serve a cached projection (bounding queries-per-second under an incident
    traffic spike). Private pages bypass the cache; both the slug and
    custom-domain routes benefit; staleness is bounded by the TTL.

### Added
- **Multi-tenancy Phase 6 — ingest enforcement behind `RAMPART_MULTI_ORG`.**
  Opt-in env flag (default **off** → existing installs byte-identical). When set,
  a telemetry-ingest request whose token doesn't match a per-org `ingest_keys`
  row is rejected with `401 Unauthorized` instead of falling back to the Default
  org — so un-keyed or wrong-keyed OTLP / RUM / profiles / Prometheus traffic
  can't land in (or be read from) the Default org. Turn it on only after minting
  a per-org ingest key for each sender. Sentry DSN, agent, and push-token ingest
  carry their own org and are unaffected. See `docs/MULTITENANCY.md`.

### Notes
- The cookie/session path is deliberately left graceful (no hard 403 on a stale
  active org): the org-switch endpoint is membership-gated, and a revoked
  membership falls back to the user's Default org rather than locking them out of
  every request (the switch endpoint included). Postgres RLS remains deferred.

---

## [0.145.0] — 2026-06-18

### Changed
- **Multi-tenancy Phase 6 (safe-reversible core): `org_id` enforcement begins.**
  Behaviour-identical for a single-org (Default-only) install — the silent
  cross-org fallbacks are NOT touched yet (that's the held, flag-gated part).
  - Migration `0112`: `SET NOT NULL` + `DROP DEFAULT` on the `org_id` column of
    all 30 tenant-root tables. A write that forgets to stamp `org_id` now fails
    loud (constraint error) instead of silently re-filing into the Default org.
    Every writer already stamps explicitly (Phase 4), so single-org is
    unaffected. Reversible (`DROP NOT NULL` / `SET DEFAULT`).
  - Migration `0113`: per-org uniqueness — `tags.name` and
    `notification_templates.name` swap their global `UNIQUE(name)` for
    `UNIQUE(org_id, name)`. All slug / DSN / token / domain / email constraints
    stay global. Degenerates to the same invariant in single-org.
  - Bearer (API-key) requests now resolve the org from the key's own
    `api_keys.org_id` instead of a hard-coded Default. Single-org keys are all
    Default, so this is behaviour-identical; it pins a key to its minting org.

### Notes
- RLS and the `RAMPART_MULTI_ORG` fallback-tightening (revoked-membership /
  ingest key-miss) remain deferred — app-level `WHERE org_id` already ships and
  is tested; see `docs/MULTITENANCY.md`.

---

## [0.144.1] — 2026-06-18

### Fixed
- **Monitor detail "Config" tab no longer white-pages.** `ConfigPanel`
  referenced the parent's `bumpMonitor` closure (via the Tags card's
  `onChanged`) but only received `monitor` as a prop — a `ReferenceError` that
  crashed the whole view when the Config tab was opened. `bumpMonitor` is now
  threaded in as an `onChanged` prop.

---

## [0.144.0] — 2026-06-18

### Added
- **Multi-tenancy — Phase 5-10: ingest-key management API + UI (Phase 5
  complete).** `GET/POST/DELETE /v1/ingest-keys` (admin-gated, scoped to the
  caller's active org via Phase-4e per-org RBAC) to mint, list and revoke the
  per-org ingest keys introduced in 5-0 — `POST` returns the plaintext token
  exactly once; `DELETE` is org-scoped (a cross-org id is a 404, not a 403).
  New Settings → **Ingest keys** view (mirrors API keys): list with kind +
  allowed-origins + last-used, a create modal (kind select; RUM keys expose an
  allowed-origins field), and a token-shown-once reveal/copy box with a `curl`
  example. Creates/deletes are audited. New `ingest_keys_api` tests
  (admin CRUD, editor-forbidden, cross-org-not-listed). **This completes
  Phase 5** — the ingest tier is fully tenanted: OTLP/Prometheus/RUM/profiles
  resolve their org from an ingest key (or fall back to Default), the agent and
  Sentry-DSN paths carry their own org, all telemetry reads + scheduler ticks
  are org-scoped, and operators manage org keys in the UI. The only remaining
  MT phase is P6 (enforcement flip). See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.143.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 5 read-scoping (5-7/8/8b/9): telemetry reads are
  org-scoped.** Every telemetry-tier read/search/aggregate now filters
  `org_id`, mirroring the Phase-3 management read-filtering — so once ingest is
  tenanted (5-1..4) one org never sees another's telemetry:
  - **logs + traces** — `query_logs`/`level_counts`/`histogram`/`list_services`
    and `list_traces`/`get_trace_spans`/`operation_stats`/`operation_trend`;
    **`service_map` self-join is org-bound on BOTH sides**; trace_id/span_id
    pivots filter org (no longer globally unique across orgs).
  - **metrics** — `list_series`/`range_query`/`latest`/`baseline`; the
    `/v1/metrics` `series`/`query` read handlers gained `OrgContext`.
  - **scheduler reads** — `metric_rules`, `telemetry_rules`, `detection`, and
    `slos` `evaluate_tick` now scope their telemetry reads to the rule/SLO's org
    (the ErrorRate project lookup joins `error_projects` by per-org name).
  - **RUM + profiles + error trace-pivot** — all read handlers scoped (incl.
    `flamegraph_one` / `fetch_folded` by id and `issues_for_trace`).
  `org_id` added to the `MetricRule`/`TelemetryRule`/`Slo`/`DetectionRule` core
  structs (+ their SELECTs). Composite `(org_id, time)` indexes on
  logs/spans/metric_samples/rum_events/profiles (migration 0111). Retention
  `prune` deletes stay system-wide by design. Behaviour-identical for a
  single-org install. New cross-org read-isolation test
  `reads_are_isolated_per_org`; full db+api+scheduler suite green (450 tests).
  Also: the remaining ingest `// P5` markers are now permanent comments
  (self-metrics + the CLI importer deliberately use the Default org; the Sentry
  DSN path is org-correct by project inheritance). **Ingest-stamp (5-1..6) +
  read-scope (5-7..9) complete; only the ingest-key management UI (5-10)
  remains.** See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.142.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 5-4: agent metric push stamps the agent's org.** The
  agent wire-protocol metric push now stamps pushed samples with the **agent's**
  org (resolved from the agent token via `lookup`) instead of Default. `org_id`
  is threaded onto the core `Agent` struct + `AgentRow` and every agent read
  (`list`/`get`/`lookup`/`create` RETURNING, with a non-null `org_id!`
  override). Retired the agent-push `// P5` marker. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.141.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 5-3: RUM beacons + browser-error auto-provision are
  org-aware (with origin-binding).** `/rum/v1/events` and `/rum/v1/errors` now
  resolve the org from the (public, `?k`) RUM key via `resolve_ingest_org_origin`
  and stamp beacons / auto-created error projects with it. Because the RUM token
  necessarily ships in the browser snippet, a key may carry `allowed_origins`:
  when set, the request `Origin` MUST match or the beacon is rejected (401),
  preventing a leaked/forged key from misattributing data cross-org.
  `error_tracking::find_or_create_by_name` is now org-scoped (lookup + create),
  so two orgs can each have an app named e.g. "web" without colliding. New tests
  `rum_origin_binding_and_org_stamp` (+ org-scoped find_or_create coverage).
  See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.140.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 5-2: Prometheus remote_write + profiles stamp the
  resolved org.** `/prom/write` and all three profile ingest formats (folded /
  pprof / OTLP) now resolve the owning org via `resolve_ingest_org` and stamp
  metric samples / profiles with it instead of the hard-coded Default (the
  profiles `store()` helper gained an `org` param threaded from each handler).
  Org-keyed ingest lands in that org; token-less stays Default. Retired the
  remaining metric/profile `// P5` markers. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.139.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 5-1: OTLP logs + traces stamp the resolved org.**
  `/otlp/v1/logs` and `/otlp/v1/traces` now resolve the owning org via
  `resolve_ingest_org` (which gates auth exactly as `require_telemetry_token`
  did) and stamp ingested logs/spans with it instead of the hard-coded Default.
  An OTLP client presenting an org-scoped ingest key lands in that org;
  token-less ingest still falls back to Default (single-org unchanged). New
  end-to-end test `otlp_logs_stamp_org_from_ingest_key`. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.138.0] — 2026-06-18

### Added
- **Multi-tenancy — Phase 5-0: per-org ingest credentials (foundation).** New
  `ingest_keys` table (migration 0110) + `rampart-db::ingest_keys` repo
  (create/find_by_token/touch_last_used/list_for_org/delete), generalizing the
  per-status-page `ingest_tokens` to per-**org** keys, plus a
  `resolve_ingest_org(pool, headers, query_k) -> OrgId` helper. A telemetry
  client (OTLP / Prometheus remote_write / RUM / profiles) presents the key in
  the same `Bearer` / `X-Rampart-Token` / `?k` slot the global telemetry_token
  uses today; a hit resolves the owning org, a **miss falls through to the
  legacy global-token gate verbatim and lands on the Default org** — so
  single-org / existing global-token installs are byte-for-byte
  behaviour-identical, and per-org isolation activates only once an operator
  mints org-scoped keys. Keys carry an optional `allowed_origins` for RUM
  origin-binding (Phase 5-3). This slice adds capability only — no ingest path
  is rewired yet (that's 5-1+). See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.137.1] — 2026-06-18

### Fixed
- **Encryption-at-rest no longer breaks live alerting.** The monitor-flip
  notifier fan-out (`routing::resolve_channels_for_monitor`) returned the
  channel `config` **without decrypting** the secrets-at-rest envelope — so
  with `RAMPART_SECRET_KEY` set (the secure default), real down/up alert
  deliveries failed with `missing field url`, while `/test`, digest and
  scheduled-report paths (which go through `notifications::get`) worked. It now
  decrypts via `secrets::open` like every other channel read path, so live
  alerting works with encryption-at-rest enabled. Regression test
  `flip_path_resolve_decrypts_channel_config`. Found by the new
  `examples/everything` live demo.

---

## [0.137.0] — 2026-06-18

### Added
- **Multi-tenancy — Phase 4g: org switcher + Organizations admin UI (Phase 4
  complete).** The frontend now exposes multi-tenancy: an **org switcher** in
  the nav drawer (shown only when the caller belongs to >1 org) that calls
  `POST /v1/orgs/{id}/switch` and reloads so the per-org role + scoped data
  refresh; and an **Organizations** settings page (`#/organizations`) to list
  the caller's orgs, create one (becoming its Admin), and — when the caller is
  an Admin of the selected org — manage members (add by email, change role,
  remove, with last-admin protection surfaced) and rename. New `api.orgs.*`
  client group. Coherence fix: `/v1/auth/me` now returns the caller's
  **active-org** role (resolved like `require_session` — `member_role(active)`
  → Default → global), so the SPA's role-gated UI matches Phase-4e enforcement
  (`user.is_admin` stays the global flag for the 2FA policy). New test
  `me_reports_active_org_role_not_global_role`; frontend vitest green.
  **This completes Phase 4** (4a write-stamping · 4b primitives+role-mirror ·
  4c `/v1/orgs` API · 4d switcher+me() · 4e per-org RBAC · 4f OIDC→org · 4g
  UI). Multiple orgs are now fully usable; the Phase-6 enforcement flip (NOT
  NULL / drop Default fallback / RLS) remains the only held MT work. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.136.0] — 2026-06-18

### Added
- **Multi-tenancy — Phase 4f: OIDC → org mapping.** A new optional
  `RAMPART_OIDC_ORG_CLAIM` env var names a userinfo claim (e.g. `groups`, a
  custom `org`, or Google's hosted-domain `hd`) that maps an SSO identity to
  org(s) **by slug**: at login each claim value is slug-normalised (lowercase,
  non-`[a-z0-9]` runs → `-`, so `"Acme Corp"`→`acme-corp`, `"acme.com"`→
  `acme-com`), matched to an existing org, the user is granted membership (at
  `RAMPART_OIDC_DEFAULT_ROLE`), and the **first** match becomes the session's
  active org (Phase 4e then scopes the user there with that role). Re-evaluated
  idempotently on every login (memberships re-sync). Unmatched values are
  ignored — **no auto-create, no deny** — and the user falls back to the
  Default org. **When `RAMPART_OIDC_ORG_CLAIM` is unset, behaviour is exactly
  as before** (provision into Default, active org unset); the `email_verified`
  gate and first-user-admin bootstrap are unchanged. Claim resolution is a
  pure, unit-tested function (`normalize_slug` + `claim_org_slugs` handle
  string/array/missing/wrong-shape). See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.135.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 4e: per-org RBAC.** `require_session` (cookie path)
  now scopes each request to the session's **active org** and resolves the
  caller's role **in that org** from `org_members`, instead of always using the
  global `users.role`. The effective per-org role is written onto `user.role`,
  so the existing RBAC guards (`require_admin`/`require_editor`/
  `require_write_or_readonly_get`, all reading `user.role`) enforce per-org
  permissions with no guard changes. This makes the 4d org switcher **effective**
  — switch into an org where you're Readonly and writes 403; switch back to one
  where you're Editor/Admin and they succeed. Non-member of the active org
  (membership revoked mid-session) or unset active org → **Default-org
  fallback** (never locks anyone out). `user.is_admin` stays the GLOBAL flag
  (the 2FA-enforcement policy + global-admin surfaces key off it). The bearer/
  API-key path is unchanged (role from the key's scope; Default org).
  Behaviour-identical for a single-org install (the 4b mirror guarantees
  `member_role(Default) == users.role`). New test `per_org_role_gates_writes`
  (same user: Editor-write-ok in Default, Readonly-403 in the switched org).
  Full api+db suite green. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.134.0] — 2026-06-18

### Added
- **Multi-tenancy — Phase 4d: org switcher + `/v1/auth/me` enrichment.**
  `POST /v1/orgs/{id}/switch` persists the caller's active org
  (`sessions.active_org_id`) for the current cookie session — gated on
  membership (switching into an org you don't belong to → 404; bearer/API-key
  callers have no session cookie → 401). `/v1/auth/me` now returns the caller's
  `orgs` list + the resolved `active_org_id` (Default-org fallback when unset),
  so the SPA can render an org switcher. Behaviour-identical for now — the
  active org becomes the one that *scopes resources* in Phase 4e (where
  `require_session` resolves `OrgContext` from `active_org_id`). New test
  `switch_active_org_and_me_reflects_it`. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.133.0] — 2026-06-18

### Added
- **Multi-tenancy — Phase 4c: `/v1/orgs` org-CRUD + membership API.** New
  `routes/orgs.rs` mounted in the self-service subtree (no global role layer —
  authorization is **per-org**, keyed on the path org id, done in-handler):
  - `GET /v1/orgs` — the caller's own orgs · `POST /v1/orgs` — any authenticated
    user creates an org and atomically becomes its Admin (`create_with_owner`;
    duplicate slug → 409; slug validated `^[a-z0-9-]{2,40}$`).
  - `GET /v1/orgs/{id}` + `GET /v1/orgs/{id}/members` — any member (Readonly+).
  - `PATCH /v1/orgs/{id}` (rename) + `POST/PATCH/DELETE …/members[/{uid}]` —
    org **Admin** only.
  Authorization helper `require_org_role`: non-member → **404** (hides org
  existence, IDOR-safe), member-but-under-privileged → **403**, via a new
  `Role::rank()`/`Role::at_least()` ordering (Admin>Editor>Readonly).
  **Last-admin protection**: demoting or removing an org's final Admin → 409.
  Member-add resolves an existing user by email (unknown → 404; never creates
  users). No org-delete (FK `ON DELETE RESTRICT` on ~30 `org_id` columns). New
  `orgs::list_members_detailed` (JOIN users for email/name) +
  `users::by_email`. 5 integration tests (`orgs_api.rs`). The org switcher that
  makes a non-Default org *active* lands in 4d. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.132.0] — 2026-06-18

### Added
- **TLS certificate-expiry monitoring.** HTTPS HTTP monitors already captured a
  cert snapshot (`cert_days_left` / `cert_subject` / `cert_checked_at`) but it
  never affected status. Two new monitor fields make it actionable:
  `check_cert` (opt-in, default off) and `cert_expiry_days` (warn threshold,
  default 14, range 1..=365; migration `0109_cert_expiry_opts.sql`). The
  decision is a pure, unit-tested `rampart_core::monitor::cert_adjusted_status`:
  with `check_cert` on, an **expired/invalid** cert marks the monitor **Down**
  (unless `ignore_tls`), and a **valid-but-near-expiry** cert downgrades
  **Up→Warn**; a hard HTTP failure stays Down (the cert never upgrades it), and
  with `check_cert` off behaviour is identical to before. The standalone `Tls`
  monitor kind now honours `cert_expiry_days` (legacy `warn_days` config still
  wins) and the scheduler refreshes a cert snapshot for `Tls` monitors so the
  detail panel renders. Frontend: an "Also monitor TLS certificate expiry"
  checkbox + threshold input on https HTTP monitors (and a threshold on `Tls`
  monitors), plus cert subject / days-left / last-checked on the monitor detail.
  Note: an opted-in HTTP monitor does a TLS handshake every tick so status
  reflects live cert state.

---

## [0.131.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 4b: org-membership primitives + the
  users.role↔Default-membership mirror.** Additive `rampart-db::orgs` helpers
  for the upcoming org CRUD / switcher / OIDC work: `update` (rename),
  `get_by_slug` (the OIDC claim→org key), `remove_member`, `count_admins`
  (last-admin protection), and `create_with_owner` (atomic org-create +
  creator-as-Admin in one tx). **Critical correctness fix:** `users::set_role`
  and `set_admin` now also upsert the user's **Default-org membership** with the
  new role, in the same transaction — previously they wrote only `users.role`,
  so the `org_members` row went stale after any post-creation role change. Phase
  4e sources the per-org effective role from `org_members`, so without this
  mirror single-org RBAC would enforce a stale role; the mirror keeps the
  Default-only install behaviour-identical. Pure-additive + the mirror; no API
  surface change. New tests incl. `set_role_mirrors_default_membership`,
  `create_with_owner_makes_creator_admin`, `rename_and_get_by_slug`,
  `remove_member_and_count_admins`. Full `rampart-db` + `rampart-api` suite
  green. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.130.0] — 2026-06-18

### Changed
- **Multi-tenancy — Phase 4a: per-INSERT org-stamping.** Every tenant-root
  create/insert now stamps `org_id` EXPLICITLY instead of relying on the
  migration-0108 column DEFAULT, so the write path is correct the moment a
  second org exists (Phase 3 had org-scoped every read; creates were
  deliberately left on the DEFAULT until now). Each `rampart-db` create/insert
  fn takes an explicit `org_id: OrgId` parameter (never from a request body —
  prevents org-spoofing). Management creates (monitors, groups, presets,
  templates, tags, notifications, notification/incident templates, escalation
  policies, on-call, silences, metric/telemetry/detection rules, SLOs, status
  pages, scheduled reports, API keys, agents, proxies, error projects,
  maintenance windows, deploy markers) thread `org.org_id` from the request's
  `OrgContext`. The **authenticated** `/v1/metrics/ingest` path stamps the
  caller's org. Token-less ingest (OTLP logs/spans, Prometheus remote-write,
  RUM, profiles, agent metric push, self-metrics, the import CLI) stamps the
  Default org with a `// P5` marker (per-org ingest credentials land in Phase
  5). `delivery_log::record` derives `org_id` from the related notification via
  `COALESCE((SELECT org_id FROM notifications WHERE id=$), DEFAULT)`. Bulk
  UNNEST inserts (logs/spans/metric_samples) stamp via `ARRAY_FILL`.
  Behaviour-identical for the single-org install today. New regression test
  `org_write_stamping.rs` proves creates stamp a non-Default org (not the
  column DEFAULT). First slice of Phase 4 (org CRUD + switcher + OIDC→org +
  per-org RBAC). See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.129.0] — 2026-06-18

### Security
- **Multi-tenancy — Phase 3u: org-gate the remaining monitor-keyed reads.** The
  flagged tail of the audit sweep — three monitor-keyed heartbeat reads that
  lacked the `monitors::get(id, org)` gate their `rollups` / `uptime_history`
  siblings already had, plus the on-call "who's on now" read:
  - `GET /v1/monitors/{id}/reliability`, `/heartbeats`, `/heartbeats.csv` now
    gate the monitor's org first (cross-org monitor id → 404).
  - `GET /v1/on-call-schedules/{id}/current` gates via `on_call::get(id, org)`;
    the unscoped `current_target` evaluator (also used by the notifier) is
    unchanged.
  Behaviour-identical for a single-org install. New test
  `monitor_heartbeat_reads_isolated`; the cross-org isolation suite is now 11
  tests. **This closes the entire Phase-3 audit sweep (3n–3u).**

### Fixed
- **delivery-log CSV export test.** `export_csv_returns_text_csv_with_header`
  drove the router directly, bypassing the session layer — so since the export
  handler became org-scoped (3e, v0.113.0) its `Extension<OrgContext>` extractor
  rejected with a 500 and the test had been silently red. The test now injects a
  Default-org `OrgContext` by hand, mirroring `require_session`. Surfaced by the
  first full-lib-suite run of the sweep; the whole `rampart-api` + `rampart-db`
  suite is green.

---

## [0.128.0] — 2026-06-18

### Security
- **Multi-tenancy — Phase 3t: org-scope the detection findings feed.** Findings
  carry no `org_id` — they inherit the owning rule's (NOT-NULL `rule_id`). The
  rule CRUD was org-scoped back in 3e, but the findings triage surface wasn't:
  - `GET /v1/detection-rules/findings` listed every org's findings → now
    `list_findings_for_org` joins `detection_findings → detection_rules` and
    filters `WHERE r.org_id`.
  - `POST /v1/detection-rules/findings/{id}/ack` acked any org's finding by id
    → now gated by `finding_in_org` (same join), so a cross-org finding id is a
    404.
  The unscoped `list_findings` and the SIEM exporter `fetch_since` stay as-is
  for system/test callers. Behaviour-identical for a single-org install. New
  integration test `detection_findings_isolated`. **This closes the main
  Phase-3 audit sweep (3n–3t)** opened after the 3m "complete" claim proved
  premature — incidents, error-tracking, bulk monitor ops, junctions,
  tag-routing, escalation episodes and detection findings are now all
  org-gated, with a 10-test cross-org isolation suite. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.127.0] — 2026-06-18

### Security
- **Multi-tenancy — Phase 3s: org-gate the escalation episodes.** Episodes
  carry no `org_id` — they inherit the owning policy's (NOT-NULL `policy_id`).
  The dashboard + ack endpoints ignored org:
  - `GET /v1/escalation-policies/episodes` listed every org's open episodes
    (monitor + rule subjects) → now `list_open_for_org` joins
    `escalation_episodes → escalation_policies` and filters `WHERE p.org_id`.
  - `POST /v1/escalation-policies/episodes/{id}/ack` (subject-agnostic ack by
    episode id) → now gated by `episode_in_org` (same join) so a cross-org
    episode id is a 404.
  - the monitor-keyed `GET/POST /v1/monitors/{id}/escalation[/ack]` → now gate
    through `monitors::get(id, org)` first.
  The unscoped `list_open` / `ack_episode` / `ack` / `open_for_monitor` db fns
  stay for the scheduler's advance scan + tests. Behaviour-identical for a
  single-org install. New integration test `escalation_episodes_isolated`. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.126.0] — 2026-06-18

### Security
- **Multi-tenancy — Phase 3r: org-gate the tag-routing surface.** The whole
  `routes/routing.rs` module (folder↔tag, folder↔channel, channel↔tag, and
  per-monitor channel excludes, plus the resolved-`effective-channels` read)
  never extracted `OrgContext` and keyed purely on caller-supplied folder /
  channel / monitor / tag ids — so an editor could read or rewrite another
  org's alert routing (which channels a folder notifies, which tags route
  where, per-monitor excludes). All 13 handlers now gate through the org-scoped
  root getters before touching a routing junction: the folder via a new
  `monitor_groups::in_org` (EXISTS gate — there's no single-id group getter),
  the channel via `notifications::get`, the monitor via `monitors::get`, the tag
  via `tags::get`; both named ends are checked, so a cross-org id on either side
  is a 404. `routing::resolve_channels_for_monitor` itself stays unscoped — the
  notifier calls it at alert time and must see the full resolution — but the
  `effective-channels` HTTP handler now gates its monitor first. Behaviour-
  identical for a single-org install. New integration test
  `tag_routing_isolated`. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.125.0] — 2026-06-18

### Security
- **Multi-tenancy — Phase 3q: org-gate the monitor-attached junctions.** The
  attach/detach + list endpoints that wire a tag or notification channel to a
  monitor (and the monitor↔monitor dependency edges) keyed purely on
  caller-supplied ids with no org check, so an editor could read or rewire
  another org's monitor relationships:
  - `GET/POST/DELETE /v1/monitors/{id}/tags[/{tag}]`,
    `/v1/monitors/{id}/notifications[/{nid}]`,
    `/v1/monitors/{id}/dependencies[/{parent}]`
  - the per-monitor arms of `POST /v1/monitors/bulk` (AddTag / RemoveTag /
    AttachChannel / DetachChannel)
  Each now gates through the org-scoped root getters before touching the
  junction: the monitor via `monitors::get(id, org)`, and the other end (tag /
  channel / parent-monitor) via `tags::get` / `notifications::get` /
  `monitors::get` — so BOTH ends must belong to the caller's org or it's a 404.
  The bulk handler validates the action's tag/channel once up front and the
  monitor per row. No db-signature changes (the internal monitor hydration and
  the seed path that call the junction fns directly are untouched).
  Behaviour-identical for a single-org install. New integration test
  `monitor_junctions_isolated`. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.124.0] — 2026-06-17

### Security
- **Multi-tenancy — Phase 3p: org-scope the bulk monitor operations.** The two
  id-list / by-tag bulk endpoints took no `OrgContext` and resolved monitors by
  id (or tag) with no org filter, so an editor could preview + mutate
  (interval / timeout / active / group / tags) any org's monitors by id, or
  pause/resume every monitor carrying a tag across all orgs:
  - `POST /v1/monitors/bulk-edit` (+ `?dry_run`) — `bulk_edit` /
    `bulk_edit_preview` now resolve each id `WHERE id = $ AND org_id = $`; an id
    in another org is reported in the existing `skipped` bucket, never read or
    mutated (the dry-run preview likewise can't see it).
  - `POST /v1/monitors/bulk-by-tag` — `set_active_by_tag` now flips `active`
    only on monitors `WHERE org_id = $` carrying the tag.
  Behaviour-identical for a single-org install. New integration test
  `bulk_edit_skips_cross_org_monitors` (a cross-org id in the batch is counted
  skipped and the row is provably untouched). See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.123.0] — 2026-06-17

### Security
- **Multi-tenancy — Phase 3o: org-scope the error-tracking surface.**
  `error_projects` is a tenant-root with its own `org_id`, and its issues /
  events / histograms / source maps inherit it, but only `recent_open_issues`
  (the dashboard feed, 3l) was scoped. The full admin surface — project
  `list` / `update` / `delete`, per-project issue list + histogram + source-map
  list/upload/delete, and the top-level `/v1/error-issues/{id}` operations
  (detail / stats / affected-users / events / resolve / ignore / unresolve /
  assign) — acted on a project or issue id with no org check. They now gate:
  `error_tracking::list/update/delete` filter `WHERE org_id`, and two
  404-gates (`project_in_org`, `issue_in_org`) front every project- and
  issue-keyed handler (the issue gate joins `error_issues → error_projects`).
  The DSN-keyed Sentry ingest (`get_opt`), the RUM auto-provision +
  event-record hot path (`find_or_create_by_name` / `record_event`), retention
  `prune`, and `issues_for_trace` (trace↔error correlation) stay intentionally
  unscoped; `assignable_users` (the assignee directory) is deferred to P4 with
  the org-membership model. Behaviour-identical for a single-org install. New
  integration test `error_projects_isolated_across_orgs`. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.122.0] — 2026-06-17

### Security
- **Multi-tenancy — Phase 3n: org-gate the incidents surface.** A follow-up
  audit of the Phase-3 read-filtering sweep found the 3m note ("complete for
  every request surface") was premature — several authenticated management
  surfaces still operated on a tenant-root resource (or its child) by id with
  no org check. First fix: **status-page incidents**. Incidents have no
  `org_id` of their own; they inherit the owning page's org. Previously
  `/v1/status-pages/{page}/incidents` (list/create) and the top-level
  `/v1/incidents/{id}` operations (update / delete / resolve / updates
  list+post) acted on any incident by id with no check that its page belonged
  to the caller's org. They now gate through the owning page —
  `status_pages::get(page, org)` (404 when the page is in another org) — so a
  cross-org page or incident id is a 404. The public per-incident Atom feed
  (resolved by slug), webhook auto-resolve ingest, and seed paths stay
  intentionally unscoped. Behaviour-identical for a single-org install (the
  only live org today). New integration test
  `incidents_isolated_via_owning_page`. More 3n+ surface fixes
  (error-tracking, bulk monitor ops, attach/detach junctions, tag-routing,
  escalation episodes, detection findings) follow. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.121.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3m: org-gated status-page sections + ingest tokens.**
  Closes a real cross-org gap: the status-page **section** management endpoints
  (`/{id}/sections` list/create + `/{id}/sections/{sid}` update/delete +
  monitor-section assign) and the **ingest-token** management endpoints
  (list/create/revoke/set-mapping) previously operated on a page/token id with
  no check that the owning page belonged to the caller's org. They now org-gate
  through the parent page — section handlers verify the page via
  `status_pages::get(page, org)` first, and `ingest_tokens::{delete,set_mapping}`
  scope by `status_page_id IN (SELECT id FROM status_pages WHERE org_id = $)`.
  A cross-org page/token id is now a 404. Behaviour-identical for a single-org
  install. **This completes Phase-3 org-scoped read/management filtering for
  every request surface except the deferred-with-their-phase items** (telemetry
  reads → P5, settings/audit_log → P6, `assignable_users` → P4). See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.120.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3l: org-scoped dashboard aggregates.** The
  cross-cutting "all monitors / all pages / all projects" dashboard reads now
  filter by the request's org via a join to the owning root: the monitors
  summary (`heartbeats::summary_window`) + history strip
  (`heartbeats::recent_per_monitor`) join `monitors.org_id`; the recent-incidents
  feed (`incidents::recent`) joins `status_pages.org_id`; the recent-open-errors
  feed (`error_tracking::recent_open_issues`) joins `error_projects.org_id`. A
  cross-org row can no longer surface in another org's dashboard tiles, history
  bars, or recent feeds. Behaviour-identical for a single-org install (the join
  matches every row). The org-member-scoped `assignable_users` read is deferred
  to Phase 4 (it needs the membership model, not a column join). See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.119.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3k: org-scoped notification + incident templates.**
  The notification-template library (`list`/`get`/`update`/`delete`) and the
  incident-update template library (`list`/`get`/`update`/`delete`) take an
  `org_id` and filter by it — a cross-org template id is a 404. The notifier's
  render-time template resolver (`templates::get_render_strings`, keyed off a
  channel's `template_id`) stays unscoped (no request context). `create` stays
  on the column DEFAULT (write-stamping is Phase 4). Behaviour-identical for a
  single-org install. **This completes Phase-3 org-scoped read filtering for
  every tenant-root management surface** (monitors, alerting, status pages,
  infra credentials, monitors-core, templates); the only remaining read paths
  are the telemetry tier (inert until per-org ingest auth in Phase 5),
  `settings`/`audit_log` (handled specially in Phase 6), and parent-scoped child
  reads (already org-safe via their root). See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.118.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3j: org-scoped tags, folders, presets, templates.**
  Completes the monitors-core domain: tag management (`list`/`get`/`update`/
  `usage`/`delete`), monitor-group/folder management (`list`/`update`/`delete`),
  and the monitor-preset + monitor-template libraries (`list`/`get`/`delete` +
  template instantiate) take an `org_id` and filter by it — a cross-org id is a
  404, the tag-usage counts and the folder list show only the org's rows, and
  cloning/instantiating validates the target folder/template belongs to the
  caller's org. The tag-attach/hydrate helpers (used inside monitor/channel list
  hydration) and the folder dependency-graph helpers stay parent-scoped and
  unchanged. `create` stays on the column DEFAULT (write-stamping is Phase 4).
  Behaviour-identical for a single-org install. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.117.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3i: org-scoped agents + scheduled reports.** Agent
  management (`list`/`get`/`update`/`delete`) and scheduled-report management
  (`list`/`get`/`update`/`delete`) take an `org_id` and filter by it — a
  cross-org id is a 404, and assigning an agent to a monitor now validates the
  agent belongs to the caller's org. The agent-token resolver
  (`agents::lookup`/`touch_seen`) and the scheduler report path
  (`due`/`mark_sent`/`render`) stay unscoped (auth / no-request-context).
  `create` stays on the column DEFAULT (write-stamping is Phase 4).
  Behaviour-identical for a single-org install. **This completes Phase-3
  read filtering for the infra-credentials domain** (api-keys, proxies, agents,
  scheduled-reports; `ingest_tokens` is parent-scoped via status pages and lands
  with the status-page-children pass). See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.116.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3h: org-scoped API keys + proxies.** The API-key
  management reads/mutations (`list`/`delete`) and proxy management
  (`list`/`get`/`delete`/`set_active`) take an `org_id` and filter by it — a
  cross-org id is a 404. The **auth-establishing** paths stay unscoped by
  design: `api_keys::lookup` (the bearer-token resolver — it *is* the auth and
  can't know the org before resolving the key) and the probe-routing
  `proxies::get_unscoped` (the scheduler / test-now resolving a monitor's proxy
  with no request context). `create` stays on the column DEFAULT (write-stamping
  is Phase 4). Behaviour-identical for a single-org install. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.115.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3g: org-scoped status pages.** The status-page
  *management* reads/mutations (`list`/`get`/`update`/`delete`) take an `org_id`
  and filter by it — a cross-org page id is a 404. The **public** surfaces stay
  unscoped by design: `get_by_slug`, `find_by_custom_domain` (Host-header
  routing), `public_view`, and `verify_page_password` resolve a published page
  by its public slug/host with no session, exactly as before. System callers
  (seed/import, the spawned incident email fan-out) use new `list_all` /
  `get_unscoped` siblings. `create` stays on the column DEFAULT (write-stamping
  is Phase 4); section management (parent-scoped via the page) is unchanged here.
  Behaviour-identical for a single-org install. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.114.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3f: org-scoped escalation policies.** The escalation
  policy management reads/mutations (`list`/`get`/`update`/`delete`) take an
  `org_id` and filter by it — a cross-org policy id is a 404. The episode
  lifecycle the scheduler + notifier drive (open/resolve/advance/due + the
  policy lookup at page time) stays unscoped via a new `get_unscoped` sibling,
  since it runs with no request context. `create` stays on the column DEFAULT
  (write-stamping is Phase 4); episode views (`list_open`/`open_for_monitor`/
  `ack`) are parent-scoped through their policy/monitor and unchanged here.
  Behaviour-identical for a single-org install. **This completes Phase-3
  read filtering for the entire alerting domain** (channels, rules, silences,
  SLOs, on-call, detection, delivery-log, escalations). See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.113.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3e: org-scoped detection rules + delivery log.**
  Continues the per-domain read-filtering rollout to `detection_rules` and the
  `delivery_log`: detection management reads/mutations (`list`/`get`/`update`/
  `delete`) and the delivery-log reads (`get`/`list`/`list_all` — the latter
  backs the admin CSV export) take an `org_id` and filter by it, so a caller
  only sees/edits its own org's detection rules and only exports its own
  delivery history. The detection evaluation tick (`detection::evaluate_tick`)
  stays unscoped via new `list_all`/`get_unscoped` siblings; `detection::preview`
  is unchanged (it dry-runs over the logs tier, scoped in Phase 5); `create` +
  `record` (the notifier-written delivery row) stay on the column DEFAULT.
  Behaviour-identical for a single-org install. This completes org-scoped read
  filtering for the alerting domain. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.112.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3d: org-scoped notification channels.** Continues the
  per-domain read-filtering rollout to `notifications` (alert channels): the
  management reads/mutations (`list`/`get`/`update`/`delete`/`counts_per_monitor`)
  take an `org_id` and filter by it — a cross-org channel id is a 404, and the
  per-monitor channel-count badge only counts the org's channels. The notifier
  fan-out resolves a channel to dispatch through `notifications::get_unscoped`
  (no request context); seed/import use `list_all`. `create` + the
  monitor↔channel junction (`attach`/`detach`/`for_monitor`) + `mark_fired` are
  unchanged (junction org-validation is deferred enforcement). Behaviour-identical
  for a single-org install. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.111.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3c: org-scoped SLOs + on-call schedules.** Continues
  the per-domain read-filtering rollout to `slos` and `on_call_schedules`: their
  management reads/mutations (`list`/`get`/`update`/`delete`, `list_with_snapshots`)
  take an `org_id` and filter by it — a cross-org id is a 404. The scheduler SLO
  evaluation tick (`slos::evaluate_tick`) and the on-call resolution
  (`current_channel`/`current_target`, used by the escalation/notifier path) stay
  unscoped via new `list_all` / `get_unscoped` siblings, since they must see every
  org. `create` stays on the column DEFAULT (write-stamping is Phase 4).
  Behaviour-identical for a single-org install. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.110.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3b: org-scoped alert rules + silences.** Continues the
  per-domain read-filtering rollout to `metric_rules`, `telemetry_alert_rules`,
  and `silences`: their management reads/mutations (`list`/`get`/`update`/
  `delete`, silence `list_active`/`delete`) take an `org_id` and filter by it, so
  a caller only sees/edits its own org's rules and a cross-org id is a 404 (one
  org can no longer lift another's silence). The scheduler evaluation ticks
  (`metric_rules::evaluate_tick`, `telemetry_rules::evaluate_tick`) and the
  notifier silence chokepoint (`is_silenced`) stay unscoped via new
  `list_all` / `get_unscoped` siblings — they must see every org's rules to
  evaluate them. `create` stays on the column DEFAULT (write-stamping is Phase 4).
  Behaviour-identical for a single-org install. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.109.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 3a: org-scoped monitors (read filtering begins).**
  The monitors management surface now filters by the request's `OrgContext`:
  `monitors::{list,get,update,delete,set_active,set_group,regenerate_push_token}`
  take an `org_id` and add `WHERE org_id = $org`, so a caller only ever sees or
  mutates its own org's monitors (a cross-org id is an IDOR-safe 404, not a
  leak). System/runtime callers that legitimately span all orgs — the scheduler
  probe loop, the notifier fan-out, push ingest, an agent reporting its own
  monitor, a status page resolving a linked monitor, retention/seed/import
  tooling — use new explicit unscoped siblings `monitors::list_all` /
  `monitors::get_unscoped` (secure-by-default: the plain names are the scoped
  ones). The `bulk` endpoint's monitor actions (pause/resume/delete/set-group)
  are org-scoped through the same fns, so a bulk request can't touch another
  org's monitors. Behaviour-identical for a single-org install (every row is the
  Default org and the context resolves to it, so the filter returns everything).
  First slice of the per-domain Phase-3 rollout; alerting / status-pages /
  telemetry domains follow. New db test `read_filter_isolates_orgs` proves a
  monitor in another org is invisible to the Default org. See
  [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.108.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 2: per-resource `org_id` columns (behaviour-identical).**
  Migration 0108 adds an `org_id` ownership column to all **30 tenant-root**
  tables (monitors, status_pages, notifications, error_projects, the
  independently-ingested telemetry tiers — logs/spans/metric_samples/rum_events
  /profiles — rules, escalation/on-call, silences, slos, delivery_log, api_keys,
  agents, proxies, deploy_markers, scheduled_reports, tags, …). The column is
  nullable with a constant DEFAULT of the Default org
  (`00000000-0000-0000-0000-000000000001`), so every existing and new row is
  owned by the Default org with **no table rewrite, no backfill, no locking
  scan** (metadata-only on PG 11+), and a plain `REFERENCES organizations(id)`
  (ON DELETE RESTRICT) so an org can't be dropped while it owns rows. Child
  tables (heartbeats, incidents, status_page_*, error_events, …) get no column —
  they inherit org transitively via their NOT-NULL FK to a root; join tables and
  instance-level tables (users, sessions, the tenancy machinery) are excluded by
  design. `settings` and `audit_log` are deferred (they gain org scoping via PK
  reshaping / a derived column in a later phase, not a generic nullable ALTER).
  No query filters by org yet — reads remain global — so this release is
  functionally identical to single-tenant. The root/child/global classification
  was produced and adversarially verified across all 65 tables (the leak-critical
  cases: `silences`/`slos` carry a *nullable* `monitor_id` so global silences /
  metric-SLOs are roots, not children; `delivery_log` orphans on `SET NULL` so it
  carries its own `org_id`). See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md).

---

## [0.107.0] — 2026-06-17

### Added
- **Multi-tenancy — Phase 1 foundation (behaviour-identical).** Introduces the
  tenant root and user↔org membership without changing any behaviour: a new
  `organizations` table + `org_members` join (role on the membership, the
  many-to-many model that later enables MSP "one admin across customers"), and
  `sessions.active_org_id`. Every install gets a well-known **Default org**
  (`00000000-0000-0000-0000-000000000001`); migration 0107 backfills every
  existing user into it with their current role and points every session at it.
  `users::create` now atomically seeds the Default-org membership, so the
  invariant "every user belongs to an org" holds for all creation paths. The
  auth layer resolves an `OrgContext { org_id, role }` on every authenticated
  request (cookie path from the session's active org with a Default-org
  fallback; bearer path → Default org) and attaches it to request extensions.
  No query filters by org and the RBAC guards still read `User.role`, so this
  release is functionally identical to single-tenant — it is the additive,
  reversible base for the phased rollout. See [`docs/MULTITENANCY.md`](docs/MULTITENANCY.md)
  for the full 6-phase plan and the tracked cross-tenant leak traps.

---

## [0.106.1] — 2026-06-17

### Fixed
- **Console noise on the operator's own dashboard.** The boot host-header probe
  (`GET /v1/public/status-pages/by-domain/{host}`) is fired on every bare-hash
  load to detect whether the current hostname is a status-page custom domain.
  When it isn't — i.e. the normal dashboard host — the endpoint returned `404`,
  which the browser logs as a red console error even though the JS already
  handled it as the expected "not a custom domain" answer. The endpoint now
  returns `200 null` for a non-matching host; the frontend already treats a
  falsy payload as "fall through to the dashboard" (App.jsx) / "page not found"
  (StatusPageView), so behaviour is unchanged and the console stays clean.
- **recharts `width(-1) height(-1)` warning.** The Dashboard response-time chart
  and both MonitorDetail charts use `<ResponsiveContainer>`, whose dimensions
  default to the `{-1,-1}` sentinel until its ResizeObserver fires — logging a
  spurious "width/height should be greater than 0" warning on first paint. Each
  now passes `initialDimension` matching its fixed-height parent, so the first
  render starts with positive dims and the warning is gone (the observer still
  corrects to the real size within a frame).

---

## [0.106.0] — 2026-06-17

### Added
- **Deploy markers — change-correlation for MTTR.** Point-in-time annotations
  (deploys, config changes) with a title, optional description + service scope,
  and timestamp. `POST /v1/deploy-markers` lets CI stamp a marker on each release
  (via an API key); the Metrics charts overlay them as dashed vertical lines
  (hover shows the label) so a latency or error change can be tied to "what
  shipped". A "Mark deploy" button in the Metrics view creates one ad-hoc.
  Migration `0106`; full CRUD (`GET ?hours=&service=`, POST, DELETE) verified by
  a live round-trip. Completes the post-audit quick-wins (#101).

---

## [0.105.0] — 2026-06-17

### Added
- **Detection v2: per-entity aggregation (`group_by`).** A detection rule can now
  aggregate by a log attribute — e.g. `group_by = user`, threshold 5 fires once
  per user with ≥5 matches in the window ("brute-force per account"), instead of
  one global count. The tick groups by `attributes->>group_by`, raises a separate
  finding per entity reaching the threshold (the finding records which entity),
  and applies cooldown **per entity** so a noisy entity doesn't mute alerts for
  others. Records lacking the attribute are ignored. Works with both the flat
  match and the v2 boolean condition. Migration `0105`; the rule form gains a
  "Group by attribute" field and findings show the entity. **This completes
  Detection v2** (suppression + boolean composition + per-entity aggregation).
  Verified: DB eval test (per-user threshold, entity recorded, under-threshold +
  attribute-less records ignored) + live API round-trip.

---

## [0.104.0] — 2026-06-17

### Added
- **Detection v2: boolean condition rules (AND / OR / NOT).** Detection rules can
  now match on an arbitrary boolean tree instead of the single flat AND-chain —
  e.g. `(service=auth AND body contains "failed") OR (severity≥error AND NOT
  env=dev)`. Leaf predicates: service, min-severity, body-regex, body-contains,
  attribute equality; each negatable. The tree is stored as JSONB (migration
  `0104`) and compiled to a parameterized SQL `WHERE` via a query builder (every
  leaf value is bound, never interpolated); tree size/depth are bounded and each
  regex leaf is validated on write. A missing attribute reads as absent so
  `NOT attr=val` matches records lacking the attribute (intuitive detection
  semantics, not SQL's three-valued NULL drop). Rules with no tree keep using the
  legacy flat fields unchanged. The rule form gains a "Boolean condition" mode
  with an OR-of-AND-groups builder (NOT per condition). Verified end-to-end:
  unit + DB eval tests, and a live API round-trip (create / validate-reject /
  clear).

---

## [0.103.0] — 2026-06-17

### Added
- **Detection rules: per-rule suppression / cooldown** (first of the Detection
  v2 work). A `cooldown_seconds` window stops a sustained match stream from
  raising a finding on *every* scheduler tick — after a finding fires, repeats
  are suppressed until the cooldown elapses (matches still advance the watermark,
  so they aren't re-counted). `0` = the legacy alert-every-tick behavior; new
  rules default to 300s. Editable in the rule form; migration `0103`. (Boolean
  composition and per-entity GROUP BY aggregation are the remaining Detection v2
  pieces, tracked separately.)

---

## [0.102.1] — 2026-06-17

### Security
- **Notification delivery now blocks literal-IP SSRF targets too.** The v0.101.4
  notifier guard wired every channel through an SSRF-guarded DNS resolver — but
  reqwest only invokes that resolver for *hostname* targets, so a channel URL
  pointing straight at an IP (e.g. `http://169.254.169.254/…` for cloud metadata,
  or `http://127.0.0.1`) connected without ever hitting the guard. The channel
  dispatcher now runs a central pre-flight (`rampart_ssrf::guard_url`) over every
  `http(s)` URL in a channel config — which resolves hostnames and checks
  IP-literals directly — before any send. Verified headless: loopback + metadata
  webhooks are blocked (`blocked by SSRF guard`), a public target still delivers.
  (Found via the v0.102.0 probe live-test, which exposed the same reqwest
  resolver-bypass on the probe path.)

---

## [0.102.0] — 2026-06-17

### Added
- **Three new probe kinds — Elasticsearch/OpenSearch, Vault, etcd** (41 kinds
  total). All HTTP-based health checks for common platform/SIEM infrastructure,
  built on the SSRF-guarded client:
  - **Elasticsearch / OpenSearch** — `GET {url}/_cluster/health`; green = up,
    yellow = warn (or up with `allow_yellow`), red = down. Optional basic auth.
  - **Vault** — `GET {url}/v1/sys/health`; maps Vault's status codes (200 active,
    429 standby → warn, 501 uninitialized / 503 sealed → down).
  - **etcd** — `GET {url}/health`; up when `{"health":"true"}`. Optional basic auth.
  Wired through the new-monitor wizard catalog, the structured config editor
  (username/password/allow_yellow), and migration `0102` (`monitor_kind` enum).

---

## [0.101.8] — 2026-06-17

### Changed
- **Monitor header: secondary actions collapse into a "⋯ More" menu.** The
  detail header packed up to nine buttons in one row, which crowded at laptop
  width. Primary actions (Test now, Test notifications, Maintenance now,
  Pause, Edit) stay inline; Clone, Save as template, CSV export, and Delete move
  into a "⋯ More" overflow menu (click-away to dismiss, `role=menu`). Readonly
  users — who only have CSV export — keep it inline. Verified headless at 1280px.

---

## [0.101.7] — 2026-06-17

### Fixed
- **Blank pages on many views — the real root cause (a render crash, not the
  v0.101.6 stale-chunk issue).** Seven views (Maintenance, Scheduled reports,
  API keys, Agents, Proxies, Users, Monitor templates) referenced their
  `reloadKey` state in a `useApi(..., [reloadKey])` dependency array placed
  *above* the `const [reloadKey, setReloadKey] = useState(0)` declaration.
  Because `const` bindings sit in the temporal dead zone until declared, each of
  those views threw `ReferenceError: Cannot access 'reloadKey' before
  initialization` at render and crashed to a blank page (the build accepts it —
  valid syntax — so only the running app surfaced it). Moved the declaration
  above its first use in every affected view. Reproduced + verified the fix
  headless against a seeded instance (all views now render). This also restores
  the navigation drawer on those pages, since a crashed view rendered nothing at
  all. Added a `reloadkey-order` test that fails CI if a view ever uses
  `reloadKey` before declaring it again.

---

## [0.101.6] — 2026-06-17

### Fixed
- **Blank/black page after a redeploy.** With the app open across a deploy, the
  in-memory `index.html` references the previous build's content-hashed view
  chunks (e.g. `Maintenance-ABC123.js`); navigating to a not-yet-loaded view
  then 404s on the missing chunk and the view crashes to the error screen
  (which reads as a blank/black page). Lazy views now auto-recover: a failed
  chunk import reloads the page once (guarded against a reload loop) to fetch a
  fresh `index.html` + chunk manifest. (`index.html` is already served
  `no-cache` with immutable hashed assets, so the reload always gets the current
  build.) Tip for an already-stuck tab: a hard refresh clears it.

### Security
- **Locked the CORS no-credentials invariant with a test.** Extracted the CORS
  policy into `cors_layer()` and added a test that fails if `allow_credentials`
  is ever enabled alongside the wildcard origin (a credential-leak/CSRF hole).

### Docs
- **Documented two HA composition caveats in the Helm values.** Corrected the
  background-work failover estimate (up to ~25s on ungraceful leader loss, not
  ~10s) and noted that OIDC login + the SSE stream are per-process, so OIDC
  across replicas needs ingress session affinity (sticky sessions).

---

## [0.101.5] — 2026-06-17

### Security
- **Probe HTTP connections are now SSRF-vetted at dial time (DNS-rebinding
  fix).** The HTTP/keyword/JSON-query and synthetic probes resolved + guarded
  the target host pre-flight but then connected to the original URL, leaving a
  TOCTOU window where an attacker controlling the target's DNS could answer with
  a public IP for the guard's lookup and an internal/metadata IP for the connect.
  Their clients are now built through `rampart_ssrf::guarded_client_builder()`, so
  the address actually dialed (including each manually-followed redirect hop) is
  re-vetted by the guarded resolver. The pre-flight check is kept for the clear
  "blocked by SSRF guard" heartbeat. Proxy and headless-renderer clients are
  intentionally left unguarded — they connect to trusted operator infra (often
  internal), and the target is still checked pre-flight. Completes the
  post-audit SSRF-hardening trio (0.101.3–0.101.5).

---

## [0.101.4] — 2026-06-17

### Security
- **Notification delivery is now SSRF-guarded.** Outbound webhook/notification
  HTTP went through bare, unguarded `reqwest` clients in ~128 channels, so an
  editor (or a compromised editor key) could point a channel at
  `169.254.169.254` (cloud metadata) or internal admin ports and exfiltrate via
  the delivery — the SSRF guard previously covered only the probe path. The SSRF
  guard was extracted into a shared `rampart-ssrf` crate that exposes a
  `GuardedResolver` (a `reqwest` DNS resolver vetting every address at **connect**
  time, so redirects are covered and there is no DNS-rebinding/TOCTOU window).
  All notification channels now build their HTTP client through this guarded
  resolver. The probe engine re-exports the same crate (no behavior change);
  pinning the probe path through the resolver follows in a subsequent release.

---

## [0.101.3] — 2026-06-17

First of the post-audit security-hardening releases.

### Security
- **Secrets-at-rest is no longer a silent default.** Notification-channel
  credentials are encrypted (AES-256-GCM) only when `RAMPART_SECRET_KEY` is set;
  previously a key-less install stored webhook tokens, SMTP passwords, and every
  channel's API keys as plaintext JSONB with no signal. Now:
  - startup logs a prominent `SECURITY:` warning when no key is configured;
  - `RAMPART_REQUIRE_SECRET_KEY=1` makes it fail-closed — the process refuses to
    start without a key (mirrors `RAMPART_REQUIRE_INGEST_AUTH`);
  - `/healthz` reports `secrets_at_rest: "encrypted" | "plaintext"` and `/metrics`
    exposes `rampart_secrets_at_rest_encrypted` (1/0) for ops alerting;
  - the dashboard shows an admin-only banner when secrets are stored plaintext.

---

## [0.101.2] — 2026-06-16

Fixes from an adversarial self-review of this session's changes.

### Security
- **Synthetic cookie jar is now host-scoped — fixes a cross-host cookie leak.**
  The automatic jar added in v0.100.0 replayed every accumulated cookie as a
  `Cookie` header on every step and every redirect hop without checking the
  host, so an auth/session cookie set by one host could be sent to another —
  either a later step targeting a different host, or a 3xx/open-redirect whose
  `Location` points at another origin (the SSRF guard blocks only internal IPs,
  not arbitrary external hosts). The jar is now keyed by issuing host and a
  cookie is only ever replayed to the exact host that set it.

### Fixed
- **SLO range validation no longer crashes the monitor edit modal.** A local
  variable shadowed the i18n `t()` function, so entering an out-of-range SLO
  target/window threw `t is not a function` and left the modal stuck instead of
  showing the range error. (Server-side validation was already the backstop.)
- **Editing a synthetic monitor no longer drops `body_contains` assertions with
  an empty/whitespace substring.** The config↔editor converters now round-trip
  such assertions cleanly.
- **Completed es/fr/de notification-hint localization.** Translated common nouns
  left in English mid-sentence (`username`, `from number`, `from sender`,
  `phones`, `line number`, `access token`); brand names, acronyms, and literal
  API field tokens stay verbatim.

---

## [0.101.1] — 2026-06-16

### Security
- **Re-verified the accepted `rustls-webpki` advisories (RUSTSEC-2026-0049 /
  0098 / 0099 / 0104) and refreshed the rationale.** Confirmed the documented
  debt still stands: fully clearing them would require either accepting an
  `aws-lc-rs`/cmake C crypto provider (via `rumqttc` 0.25, which still offers no
  `ring` opt-in) or dropping the MSSQL/NATS/MQTT probes — both trade away the
  deliberate pure-Rust, C-toolchain-free build. Corrected a stale note: contrary
  to the prior text, `async-nats` 0.49 *does* now expose a `ring` feature, but
  bumping it alone clears nothing while `rumqttc` and `tiberius` (still latest at
  0.12.3, no rustls-0.23 release) keep dragging the old webpki in. The advisories
  are outbound-probe-TLS only with the CRL paths unreached by default config.
  Re-confirmed `cargo tree -i aws-lc-rs|cmake|openssl` all return no matches.
  See `docs/SECURITY-DEBT.md`.

---

## [0.101.0] — 2026-06-16

### Added
- **Edit synthetic-monitor steps after creation.** The monitor edit modal now
  shows the same structured transaction-step builder as the new-monitor wizard
  for synthetic monitors — add/remove steps, edit method/URL/headers/body, and
  manage per-step `{{var}}` extractions and assertions — instead of forcing
  operators to hand-edit raw `config.steps` JSON. The step editor and its
  shape↔`config.steps` converters were extracted to a shared
  `components/SyntheticSteps.jsx` so the create and edit paths can't drift, and
  any non-step config keys are preserved across a save.

---

## [0.100.1] — 2026-06-16

### Changed
- **Notification channels: completed es / fr / de localization.** Filled the
  long-tail of ~158 untranslated channel field labels and one-line setup hints
  (e.g. Bark, Feishu, DingTalk, the SMS-gateway family, WhatsApp bridges,
  PagerTree/Squadcast/GoAlert, Mastodon, …) that were previously falling back to
  English in Spanish, French, and German. Brand names, domains, technical
  acronyms (API/URL/ARN/SID/PEM/…), and code identifiers (`chat_id`,
  `access_token`, HTTP path fragments) are intentionally left verbatim. ja/zh
  remain on the English-fallback path by design.

---

## [0.100.0] — 2026-06-16

### Added
- **Synthetic monitors: automatic cookie jar.** Multi-step synthetic checks now
  carry session state across steps without manual wiring — a `Set-Cookie` from
  any step (or a followed redirect hop) is harvested and replayed as `Cookie` on
  every later request in the run. Login → authenticated-page sequences work out
  of the box; `{{var}}` extraction remains for non-cookie state. The jar is a
  minimal in-memory name→value map (last-write-wins), so it adds no dependency
  and keeps the pure-Rust build intact.

---

## [0.99.3] — 2026-06-16

### Fixed
- Logs keyset pagination ordered only by `ts DESC` while the cursor compared
  `(ts, id)` — so rows sharing a timestamp could be skipped or duplicated across
  "Load older" pages. `ORDER BY` is now `ts DESC, id DESC`, matching the cursor.

### Tested
- Unit tests for the self-metrics rate/mean derivation (incl. counter-reset
  saturation) and for logs keyset pagination
  (`keyset_pagination_covers_all_without_overlap`, with timestamp ties).
  (The end-to-end CI smoke already runs in the `e2e` job across 5 browsers.)

---

## [0.99.2] — 2026-06-16

### Performance
- **Public status page: short-TTL cache on the projection.** Rendering a public
  page runs ~5 queries per attached monitor on an unauthenticated path, so a
  popular page during an incident (many concurrent viewers + auto-refresh) could
  hammer the DB. The non-private projection is now cached per-slug for 15s,
  collapsing a viewer burst into one rollup per window (privacy/lock checks
  still run live; expired entries self-evict). Set-based rollups to cut the
  per-miss cost are a deeper follow-up. (Audit #25.)

---

## [0.99.1] — 2026-06-16

### Changed
- Replaced the remaining `window.location.reload()` mutations in the Dashboard
  (clone / bulk / bulk-by-tag), MonitorDetail (edit-save, tag attach/detach),
  StatusPageBuilder (pages / subscribers / incidents panels), and Notifications
  views with in-place refetch (a `reloadKey` bump, or a parent callback for the
  edit/tag child components) — no more full-page flash. Completes audit #11
  (the simple list views landed in v0.95.2). The only intentional reload left is
  Security.jsx's post-2FA session refresh.

---

## [0.99.0] — 2026-06-16

### Added
- **Rampart self-metrics in the Metrics view.** A background task snapshots
  Rampart's own HTTP counters once a minute and pushes `rampart_http_requests_
  per_sec` + `rampart_http_latency_ms_avg` (`service=rampart`) into the metric
  tier, so the in-app Metrics view shows the app's **live** request rate +
  latency — not just externally-pushed series. (Rampart's full Prometheus
  exposition is still at the `/metrics` scrape endpoint.) `seed-demo` seeds these
  series so the demo shows them immediately.

---

## [0.98.1] — 2026-06-16

### Fixed
- **Accessibility: the monitor-edit and clone-into-folder modals are now proper
  dialogs** — `role="dialog"`, `aria-modal`, a focus trap that keeps Tab inside
  the dialog, autofocus on open, focus restore to the opener on close, and
  Escape to dismiss (via a shared `useFocusTrap` hook, matching the
  toast/confirm dialog host). (Audit #21.)

---

## [0.98.0] — 2026-06-16

### Added
- **Error-issue "Load older" keyset pagination.** A project's issue list can
  now page past its cap via a `(last_seen, id)` keyset cursor (`before_id`,
  resolved server-side) with a clamped `limit`, instead of a hard `LIMIT 200`.
  (Audit #23.)

---

## [0.97.0] — 2026-06-16

### Added
- **Traces "Load older" keyset pagination.** The trace list can now page past
  its cap via a `(started_at, trace_id)` keyset cursor (`before_id`, resolved
  server-side) applied in the aggregated query's `HAVING`. (Audit #24, traces —
  completes #24 with the logs pagination from v0.96.0.)

---

## [0.96.0] — 2026-06-16

### Added
- **Logs "Load older" keyset pagination.** The logs list can now page past its
  300-row cap within the selected window — a stable `(ts, id)` keyset cursor
  (`before_id`, resolved server-side so there's no client-side timestamp
  precision loss). The list query also now honours the time-window selector
  (previously only the histogram did). (Audit #24, logs.)

---

## [0.95.2] — 2026-06-16

### Changed
- The Agents, API keys, Proxies, Maintenance, Monitor templates, Scheduled
  reports, and Users admin views now **refetch their list in place** after a
  create/delete (a `reloadKey` bump) instead of a full `window.location.reload()`
  that flashed the whole shell and lost scroll/filters. (Audit #11; the
  Dashboard / MonitorDetail / StatusPageBuilder / Notifications reloads, which
  fire from nested edit contexts, are a follow-up.)

---

## [0.95.1] — 2026-06-16

### Changed
- Replaced **every** blocking `alert()` / `confirm()` / `prompt()` in the UI
  (68 sites across 25 views) with the non-blocking toast + accessible-dialog
  primitives from v0.95.0. Errors now surface as dismissible toasts; confirms
  and prompts use the focus-trapped modal — no more browser-chrome dialogs
  stealing focus or blocking the event loop. (Audit #26.)

---

## [0.95.0] — 2026-06-16

### Added
- **Shared toast + accessible-dialog primitives** (`lib/notify.js` +
  `components/Notify.jsx`, mounted in `App`): `toast(msg, kind)` for transient
  notices and promise-based `confirmDialog()` / `promptDialog()` that render an
  accessible modal — `role="dialog"`, `aria-modal`, focus trap + restore,
  Escape-to-cancel, backdrop click to dismiss. These replace the blocking
  `alert()/confirm()/prompt()` calls (foundation for audit #21 + #26).

### Changed
- The monitor "Save as template" flow now uses the inline dialog/toast
  primitives instead of `prompt()` + `confirm()` + `alert()`, completing the
  test-now + save-template halves of audit #20.
- Renamed the main nav entry from "Dashboard" to **"Overview"** so it's no
  longer confused with the separate "Dashboards" (custom dashboards) entry.

---

## [0.94.0] — 2026-06-16

### Changed
- The monitor "Test now" result is shown in an inline, dismissible banner
  instead of a blocking `alert()` — consistent with the existing
  test-notifications result panel, and it no longer steals focus. (Audit #20,
  test-now half; the save-as-template `prompt()` is folded into the upcoming
  shared-modal work.)

---

## [0.93.0] — 2026-06-16

### Changed
- The delivery log now shows the **monitor name as a link** to that monitor
  instead of a raw UUID (resolved from a one-time monitor fetch; falls back to a
  short id if the monitor is gone). Server-side filtering of the log is a
  separate follow-up. (Audit #22, display half.)

---

## [0.92.0] — 2026-06-16

### Fixed
- **On-call `…/current` now reports user shifts, not just channels.** The
  endpoint resolved only the channel ring, so a schedule rotating over *users*
  reported "nobody on call" (`null`). It now returns the combined channel+user
  ring's current target as `{kind:"channel"|"user", id}`. (Audit #10.)

---

## [0.91.0] — 2026-06-16

### Performance
- **Logs and spans now ingest in a single bulk `INSERT … UNNEST`** instead of a
  per-row loop inside a transaction. The OTLP log/trace ingest hot paths
  previously issued one round-trip per record (hundreds per batch); they now
  expand column-parallel arrays server-side in one statement. Spans keep their
  `ON CONFLICT (span_id) DO NOTHING` dedup. (Audit #18.)

---

## [0.90.0] — 2026-06-16

### Fixed
- **Logs list + CSV export now honour the time-window (`hours`) filter.** They
  previously ignored it — the histogram and level counts respected the window
  but the actual log rows always returned the newest N regardless of age, so a
  narrowed window showed stale rows. `query_logs` now bounds `received_at` to
  the window (default 24h); the trace/span pivots stay unbounded. (Audit #12.)
- **Admin incident history is bounded.** `incidents::list_all` had no `LIMIT`,
  so a page with a long incident history returned an unbounded payload. Clamped
  to 500. (Audit #19.)

---

## [0.89.0] — 2026-06-16

### Performance
- Added indexes matching the telemetry tiers' real read patterns (migration
  `0101`), which previously fell back to sequential scans: `logs (ts DESC)` +
  `logs (service_name, ts DESC)` + `logs (received_at)` for the log list and
  histogram; `metric_samples (name, labels, ts DESC)` for the metric explorer
  range query + anomaly baseline; `rum_events (url, ts DESC)` for the per-page
  RUM query. (Audit findings #13/#14/#15.)

---

## [0.88.0] — 2026-06-16

### Security
- **Notification-channel secrets are no longer exposed on read.** Channel
  `config` is decrypted by the DB layer, so the `/v1/notifications` list/get
  endpoints were handing every authenticated user — including read-only ones —
  the plaintext webhook URLs, bot tokens, API keys, and SMTP passwords of every
  channel. Secret-shaped config keys are now masked (`••••••`) on all HTTP read
  responses; an edit that leaves a masked value untouched restores the stored
  secret on write, so editing still works. The notifier reads config straight
  from the DB, so delivery is unaffected. Unit-tested.
- **Synthetic monitor SSRF via redirects closed.** The synthetic probe let
  `reqwest` auto-follow redirects, so a target could `302` to an internal
  address that the initial SSRF guard never saw. Redirects are now followed
  manually (up to 10 hops), re-resolving and SSRF-guarding every `Location`
  before connecting, with RFC-correct method/body handling.

(Both audit findings, HIGH.)

---

## [0.87.0] — 2026-06-16

### Security
- **OIDC: require a verified email before provisioning or linking an account.**
  The callback trusted the `email` claim unconditionally, so anyone who could
  register an *unverified* account at the configured IdP under a victim's
  address could log in as — or auto-provision — that victim's Rampart user
  (account takeover). The callback now rejects logins unless the provider
  asserts `email_verified` true (tolerant of bool or `"true"`/`"false"` string
  encodings). (Audit finding, HIGH.)

---

## [0.86.0] — 2026-06-16

### Security
- **Decompression-bomb guard on public ingest.** `decompress` (OTLP/RUM/Sentry
  endpoints) inflated `Content-Encoding: gzip|deflate` bodies with an unbounded
  `read_to_end` — a few KB of crafted input could expand to gigabytes and OOM
  the process. Now capped at 64 MiB; over-limit bodies are rejected (400)
  rather than read.

### Fixed
- **RDAP**: an already-expired domain stayed `Warn` forever — the
  `.max(0)` clamp floored the remaining time to 0 days. Now reports `Down` once
  the expiration event is in the past.
- **TLS**: a certificate that expired less than 24h ago reported `Warn` instead
  of `Down`, because the remaining time was truncated to whole days (rounding to
  0) before the sign check. Now branches on the raw seconds, so any past
  `not_after` is `Down`.

(All three surfaced + adversarially verified by the codebase audit.)

---

## [0.85.0] — 2026-06-16

### Added
- **React error boundary** around the view router: a render-time throw in any
  single view now shows a friendly "this view hit an error — reload / back to
  dashboard" card instead of white-screening the entire app. Keyed by route, so
  navigating away clears it. (en/es/fr/de)
- Empty-notification-channel hints in the alert-rule, detection, escalation,
  SLO, metric-rule, and error-project forms now carry a **"Go to Notifications
  →"** link (opens in a new tab, preserving the in-progress form).

### Fixed
- Dashboard monitor-table status pills (Outage/Degraded/Maintenance/Paused/
  Pending), tag tooltips, and the channel-count tooltip were hardcoded English;
  the tag `.map(t => …)` callback also **shadowed the imported `t()`** i18n
  function. Renamed the loop variable and routed all of it through `t()`
  (`dashboard.status.*` etc., en/es/fr/de). (Surfaced + verified by the audit.)

---

## [0.84.0] — 2026-06-16

### Fixed
- The dashboard hero (status headline + subtitle) and the API-error footer were
  hardcoded English that bypassed i18n — the most-visible view in the app. Now
  routed through `t()` under `dashboard.hero.*` / `dashboard.error` with
  singular/plural variants, translated in en/es/fr/de. (Surfaced by an
  automated multi-dimension audit of the codebase.)

---

## [0.83.0] — 2026-06-16

### Demo
- More depth in the deep-dive tiers: `seed-demo` now adds ~10 extra distributed
  traces (varied services/latency, a couple of error traces), ~18 RUM beacons
  across 6 pages × 3 sessions with device variety, two more profiles (a second
  `[demo] api` cpu capture so the flamegraph **diff** has two captures, plus a
  `[demo] worker` profile), and three more metric series (queue depth, error
  rate, CPU). So the trace list / service map, RUM pages, profiling pickers, and
  metric explorer all have real shape.

### Safety
- `seed-demo` now **refuses to run on a non-demo instance**: if the database
  already holds monitors that aren't `[demo]`-prefixed it's almost certainly a
  real deployment, so the seeder bails with a clear message rather than
  polluting prod with sample traces/RUM/logs. `RAMPART_SEED_FORCE=1` overrides.
  (Normal prod starts never seed — `seed-demo` is an explicit subcommand only
  the example compose stacks invoke — this just closes the accidental-run gap.)

---

## [0.82.0] — 2026-06-16

### Demo
- `seed-demo` now exercises **every** feature tier so a fresh install shows the
  whole product, not just core uptime. Added: 6 more app monitors across new
  probe kinds (gRPC, MQTT, DNS, TLS, plus auth/search HTTP), a multi-step
  **synthetic transaction** (homepage → login → checkout with extract +
  assertions), a **cron-scheduled** push monitor, an **on-call** rotation, a
  two-step **escalation policy**, an upcoming **maintenance window** (attached
  to two monitors), a **silence**, a **metric alert rule**, one **telemetry
  rule of each kind** (log-volume / trace-latency / RUM-LCP / profile-samples /
  error-rate), two more **SIEM detection rules**, two status-page **incidents**,
  a status-page **subscriber**, an **API key**, a remote **probe agent**, an
  outbound **proxy**, three **tags** (attached to monitors), and a **scheduled
  report**. Every addition is best-effort and idempotent on a fresh demo
  folder. Verified end-to-end: all tiers populate and render.

---

## [0.81.0] — 2026-06-16

### Fixed
- All 128 notification channel-picker descriptions (the catalog `hint:` blurbs)
  now route through `t()` (`notif.kind.<id>.hint`) instead of hardcoded
  English. The 21 most-used channels (Slack, Discord, Teams, Telegram, Email,
  Webhook, PagerDuty, Opsgenie, Signal, Matrix, Sentry, ntfy, Pushover, … ) are
  translated in es/fr/de, preserving URLs / API endpoints / brand names; the
  long-tail descriptions fall back to English and are now translatable without
  code changes.

---

## [0.80.0] — 2026-06-16

### Fixed
- Localized the safe-to-translate `<select>` options and the plain field-hints
  in the notification-channel forms (en/es/fr/de): Mastodon visibility
  (public/unlisted/private/direct), SMTP encryption descriptors, Zulip
  destination type, the "+ tag…" picker, and the SMS-metering / Apprise-sidecar
  hints. Options whose visible text **is** the submitted value (HTTP methods,
  `SEV-1…5`, `P1…5`, Datadog/region codes, platform ids, severity words) are
  intentionally left as-is — translating them would change the value sent to
  the provider.

---

## [0.79.0] — 2026-06-16

### Demo
- `seed-demo` now seeds a representative `[demo] api` CPU flamegraph, so the
  Profiling view shows a real merged tree on a fresh install instead of "no
  profiles yet". Goes through the same folded-stack storage the live ingest
  path uses.

---

## [0.78.0] — 2026-06-16

### Added
- Livelier interactions across the app (`src/index.css`): buttons lift with a
  soft shadow on hover (primary/accent buttons get a tinted glow) and press
  down on click; cards lift gently; list rows show an inset accent edge on
  hover. Ghost/icon buttons stay flat. All additive, scoped under `.rampart`,
  and disabled under `prefers-reduced-motion`.
- Flamegraph (Profiling) UX: the info bar now shows **% of parent** next to %
  of total; **Esc** resets the zoom to the full profile; clicking the top
  (zoomed-root) frame zooms back out one level; and an inline hint spells out
  the zoom/reset controls.

---

## [0.77.0] — 2026-06-16

### Added
- Global interaction polish (`src/index.css`): smooth color/shadow transitions
  on buttons, inputs, selects and cards; a keyboard-only `:focus-visible` accent
  ring; and a subtle button press response — applied app-wide on top of each
  view's scoped styles, with a `prefers-reduced-motion` opt-out. Additive only,
  so no view's existing look changes at rest.

### Docs
- Corrected the notification-channel count repo-wide: the README badge,
  headline, "why", architecture comment, and several docs said **130**, but the
  notifier ships **128** channel adapters (the `ChannelKind::Custom` enum
  variant is an internal placeholder with no adapter, and the README's own list
  already enumerated 128). `docs/NOTIFICATIONS.md` no longer double-counts the
  Apprise gateway and Generic Webhook. The "38 probe kinds" count is correct
  (the 39th `MonitorKind`, `Synthetic`, is a multi-step composite, not a probe).
- Fixed a broken README link to `docs/MAINTAINERS.md` (was pointing at repo
  root).

---

## [0.76.0] — 2026-06-16

### Fixed
- The notification-channel config forms had ~150 hardcoded English field
  labels (across every channel kind) that bypassed i18n. All are now routed
  through `t()` under the `notif.f.*` namespace, with es/fr/de translations for
  the common terms (Webhook URL, Password, Server, Port, Region, Priority, …)
  and an English fallback for purely technical identifiers (DSN, ARN, SAS key,
  …) so they can be translated later without code changes. The inline
  "· optional" suffix now reuses `common.optional`.

---

## [0.75.0] — 2026-06-16

### Fixed
- More hardcoded English in the monitor detail view now routes through `t()`
  and is translated in en/es/fr/de: the "Reliability window" / "Burn-down
  window" chart `aria-label`s, plus prose ("No monitor selected", "Monitor not
  found", "Back to dashboard", "Pending first check", "All samples", "Attach an
  existing channel", "Error budget burn-down", "Depends on"). The modal close
  button reuses `common.close` instead of a literal.

---

## [0.74.0] — 2026-06-16

### Fixed
- Fourteen `title` tooltips in the dashboard header/cards, the monitor detail
  view, and the push-notification button were hardcoded English bypassing
  i18n. They now route through `t()` and are translated in en/es/fr/de (new
  `common.menu`/`detach`/`color`, `nav.open`, `monitor.send_test`,
  `monitor.token_reissue_tip`, `dashboard.tip.*`, and
  `notifications.push.subscribe_tip` keys).

---

## [0.73.0] — 2026-06-16

### Fixed
- Twelve form-validation and delete-confirm messages in the monitor editor,
  notification-channel forms, and maintenance scheduler were hardcoded English
  that bypassed i18n. They now route through `t()` and are translated in
  en/es/fr/de (new `validation.*`, `maintenance.delete_confirm`, and
  `notifications.channel.delete_confirm` keys).

---

## [0.72.0] — 2026-06-16

### Changed
- "Maintenance now" windows are named `Manual maintenance · <monitor>` instead
  of a bare `Manual maintenance`, so dashboard and status-page rows say which
  monitor the window covers. Localized in all six languages.

### Demo
- `seed-demo` now emits ~150 time-spread background log lines (≈12.5h of
  history across four services, mostly info with ~16% warn / ~6% error) on top
  of the seven correlated lines, so the Logs view, level counts, and histogram
  look populated rather than nearly empty. Deterministic across reseeds.

---

## [0.71.0] — 2026-06-16

### Accessibility
- Icon-only buttons across 18 views now carry an `aria-label` (or reuse an
  existing `title`) so screen readers announce them: every inline Cancel (✕),
  Close, Clear, and Remove control in monitor/wizard/status-page/admin forms.
- New `common.remove` string localized in en/es/fr/de.

---

## [0.70.2] — 2026-06-16

### Tested
- Integration test for `incidents::recent` — active incidents sort before
  resolved ones across pages, newest within.

---

## [0.70.1] — 2026-06-16

### Fixed
- Dashboard recent-incidents severity mapping used non-existent style values
  (`major`/`critical`); the `IncidentStyle` enum is `danger`/`warning`/… — so a
  `danger` incident now correctly shows as an outage, not degraded.

---

## [0.70.0] — 2026-06-16

### Fixed
- **Dashboard "Recent incidents" + "Maintenance" were always empty** — both were
  hardcoded stubs (`const recentIncidents = []`). Now wired: incidents via a new
  cross-page `GET /v1/incidents/recent` (active first, then newest), maintenance
  via the existing list filtered to active/upcoming windows, soonest first.
- **Dashboard response-time chart was blank** when the busiest monitors were
  down — it ranked by raw heartbeat count, so down demo services (latency =
  null) won the slots and plotted only gaps. Now ranks by **up heartbeats that
  carry a latency**, so the chart shows monitors with real samples.

---

## [0.69.0] — 2026-06-16

### Added
- **Dashboard metric-series widget.** The sidebar now lists recently-active
  metric series (name + sample count), linking to the Metrics view — so ingested
  metrics surface on the overview alongside monitors / SLOs / errors /
  escalations. Hidden when no metrics exist. i18n en/es/fr/de.

---

## [0.68.2] — 2026-06-16

### Changed
- The **Metrics** empty state is more actionable — besides the push-gateway
  curl, it now points to `rampart-api seed-demo` (demo metrics), Prometheus
  `remote_write` (`/prom/write`), and the rampart-agent. (The view is empty only
  when no metrics have been ingested; `list_series` has no time window.) i18n
  en/es/fr/de.

---

## [0.68.1] — 2026-06-16

### Documentation
- `docs/CORRELATION.md` updated for the span→logs link, the APM-operation→traces
  pivot, and a **user-identity** section (the same app user id ties RUM loads to
  error issues — "who experienced this" across tiers).

---

## [0.68.0] — 2026-06-16

### Added
- **Span → logs.** The logs query gains a `span_id` filter, and an expanded span
  in the trace waterfall now shows the **logs emitted under that exact span**
  inline (level + body) — finer than the existing trace-level correlation.
  i18n en/es/fr/de.

---

## [0.67.2] — 2026-06-16

### Documentation
- README feature matrix refreshed to the current depth — error affected-users +
  volume histogram, the reworked trace waterfall / service-map edges / ops p95
  trend, the log volume histogram, RUM drill-down + users/browser breakdowns,
  the interactive profiler, the monitor latency SLA, and DB/MQTT/LDAP auth.

---

## [0.67.1] — 2026-06-16

### Tested
- Integration test for `traces::operation_trend` — the p95 series is in range
  for a recent operation and empty for an unknown one.

---

## [0.67.0] — 2026-06-16

### Added
- **Per-operation p95 latency trend.** Each row in the APM Operations table
  expands to a p95-latency sparkline over the window (red when trending up),
  via `GET /v1/traces/operation-trend` (`date_bin`-bucketed). A `→` link still
  pivots to that service's traces. Spot a degrading operation at a glance.
  i18n en/es/fr/de.

---

## [0.66.2] — 2026-06-16

### Security
- Audited the open advisories (`cargo audit`): all are the transitive
  `rustls-webpki` 0.102/0.101 cert-validation bugs, already accepted in
  `deny.toml` with CI passing. Made the accounting precise — the vulnerable
  webpki is pulled by **three** probe deps (rumqttc, async-nats, tiberius), and
  the fix is blocked because the only rustls-0.23 `rumqttc` (0.25) pulls
  `aws-lc-rs`/cmake, breaking the pure-Rust build. Updated `deny.toml` +
  `docs/SECURITY-DEBT.md`. No code change; no safe upgrade exists yet.

---

## [0.66.1] — 2026-06-16

### Tested
- Integration tests for the new identity aggregates: `issue_affected_users`
  (distinct users + per-user counts, anonymous events excluded) and
  `rum::user_breakdown` (grouped by user, busiest first, anon excluded).

---

## [0.66.0] — 2026-06-16

### Added
- **Error issue → affected users.** The issue detail now lists the distinct
  users hit by an issue (from the Sentry `user` context — id / email / username)
  with per-user event counts, beside the existing affected-count and
  release/environment stats. `GET /v1/error-issues/{id}/users`. i18n en/es/fr/de.

---

## [0.65.1] — 2026-06-16

### Documentation
- `docs/design/RUM.md` documents the snippet's **correlation & identity hooks**
  (`window.__rampartUser`, `window.__rampartTraceId` / `<meta traceparent>`), the
  updated beacon shape (`trace_id` / `user_id`), and the new read endpoints
  (`/page`, `/users`, `/browsers`, `/traced`).

---

## [0.65.0] — 2026-06-16

### Added
- **RUM users breakdown.** The RUM view gains a Users table — page-views and p75
  LCP per app user (from the `user_id` beacons), busiest first, so you can see
  which users have a poor experience. `GET /v1/rum/users`. i18n en/es/fr/de.

---

## [0.64.1] — 2026-06-16

### Tested
- Integration test for `rum::page_samples` (the page drill-down): a URL's recent
  loads carry the user id and vitals, and a different URL is isolated.

---

## [0.64.0] — 2026-06-16

### Added
- **RUM page drill-down + user identity.** RUM beacons can now carry an app
  **`user_id`** (migration 0100; the snippet reads `window.__rampartUser`,
  string or `{id}`). Each row in the RUM **Pages** table is clickable to expand
  the recent individual loads for that URL — **who** (user id, else session),
  browser, LCP/INP, and a trace link per load — via `GET /v1/rum/page`. Answers
  "dive into a page" and "who experienced this". `seed-demo` tags its loads with
  a demo user. i18n en/es/fr/de.

---

## [0.63.1] — 2026-06-16

### Fixed
- **Adding a monitor dependency bounced you back to the overview.** Attach/detach
  did a full `window.location.reload()`, which lost the route. They now refetch
  the dependency list in place, so you can add several dependencies in a row
  without re-navigating.

---

## [0.63.0] — 2026-06-16

### Added
- **Structured probe-config editor.** The monitor edit modal's probe config now
  has a **Form / JSON** toggle — a structured form with the known keys per kind
  (auth, latency SLA, keyword/DNS/expect, etc.) defaulting on for kinds that
  have fields, with the raw JSON editor one click away. Unlisted keys are
  preserved; invalid JSON prompts a switch to JSON mode. i18n en/es/fr/de.
- The **Max latency (ms)** SLA field now shows for **all connect-based monitor
  kinds** in the wizard (was DB/cache only) — it was always generic.

---

## [0.62.1] — 2026-06-16

### Added
- Auth fields extended to **MQTT** (username / password) and **LDAP**
  (bind DN / bind password) monitors in the wizard, written to the config keys
  those probes already read; edit-modal config hints added for both. (AMQP
  carries credentials in its `amqp://user:pass@host` URL; Kafka/NATS have no
  probe-level auth.) The `max_latency_ms` field now also shows on these kinds.
  i18n en/es/fr/de.

---

## [0.62.0] — 2026-06-16

### Added
- **Latency SLA threshold** (`config.max_latency_ms`). A check that connects and
  responds but *slower* than the threshold is now marked **down** ("slow: Nms >
  threshold") — degraded detection distinct from the hard connection
  `timeout_seconds`. Applied centrally in the probe dispatcher, so it works for
  every monitor kind; surfaced as a field on DB/cache monitors in the wizard and
  documented in the edit-modal config hints. Covered by unit tests.

### Fixed
- **Monitor drag-and-drop snapped back on drop.** The drop zone was only the
  thin group-header strip, so dropping a monitor onto a row (or the list body)
  of the target group missed and the drag reverted. The whole group bucket is
  now the drop zone.

---

## [0.61.0] — 2026-06-16

### Added
- **Database monitor auth fields.** The add-monitor wizard now exposes
  **username / password / database** fields for Postgres, MySQL, MSSQL, MongoDB,
  and Redis (Redis: username + password + DB number) — written into
  `config.{user,password,database|db}`, which the probes already read. The edit
  modal's config-JSON helper gained matching per-kind placeholders + key hints.
  Redis gained ACL **username** support (`config.user`, Redis 6+) in the probe.
  i18n en/es/fr/de.

---

## [0.60.0] — 2026-06-16

### Added
- **Dashboard active-escalations widget.** A sidebar panel lists open escalation
  episodes — "who's being paged right now" — with subject kind, current step,
  acknowledged-vs-climbing state, and how long it's been running, linking to the
  escalations view. Completes the dashboard's at-a-glance set (monitors, SLOs,
  errors, escalations). i18n en/es/fr/de.

---

## [0.59.4] — 2026-06-16

### Tested
- Extended the SLO test with an assertion on `slos::trend` (the achieved-ratio
  sparkline data), completing direct coverage of the read aggregates added this
  cycle (logs/RUM/errors/traces/SLO).

---

## [0.59.3] — 2026-06-16

### Tested
- Integration test for the enriched service map (`service_map`): per-edge call
  count, error count, and p95 latency of the callee span, plus that same-service
  parent/child pairs don't form an edge.

---

## [0.59.2] — 2026-06-16

### Fixed
- **Dashboard recent-errors widget was always empty** (since v0.53.0):
  `recent_open_issues` filtered `status = 'open'`, but issue status is
  `unresolved` | `resolved` | `ignored` — so it never matched. Now filters
  `unresolved`. Caught by the new test below.

### Tested
- Integration test for the error-tracking read aggregates added this cycle:
  the cross-project recent-open feed (`recent_open_issues`, incl. that resolving
  drops an issue) and the per-project event histogram (`project_event_histogram`
  counts every event, not just issues), over a recorded-event fixture.

---

## [0.59.1] — 2026-06-16

### Fixed
- `seed-demo` RUM beacons used `lcp_ms` / `fcp_ms` / … metric keys, but the
  beacon schema expects `lcp` / `fcp` / `inp` / `ttfb` / `load` (only `cls`
  matched) — so the demo's RUM vitals were dropped on ingest. Corrected, so the
  seeded dashboard shows real LCP/INP/etc. (Caught by the new RUM test below.)

### Tested
- Integration tests for the new RUM and logs read aggregates: the log-volume
  histogram (total + error split, service / min-severity / full-text filters)
  and level counts; the RUM browser breakdown (coarse UA classification, incl.
  the Edge-contains-Chrome case) and the recent-traced feed.

---

## [0.59.0] — 2026-06-16

### Added
- **RUM browser breakdown.** The RUM view gains a Browsers table — page-views
  and p75 LCP per browser family (Chrome / Firefox / Safari / Edge / Opera /
  Other), so a slow-on-one-browser regression stands out. `GET /v1/rum/browsers`
  (coarse UA classification in SQL, no parser dependency). i18n en/es/fr/de.

---

## [0.58.0] — 2026-06-16

### Added
- **Error-volume histogram** on the Errors project view — a 7-day bar chart of
  error events above the issue list (`GET /v1/error-projects/{id}/histogram`,
  `date_bin` bucketed), so a spike is visible before you scan the issues.

---

## [0.57.1] — 2026-06-16

### Changed
- APM **Operations** and **Service map** tabs gain a 1h / 24h / 7d time-window
  selector (were fixed at 24h), matching the logs histogram window control.

---

## [0.57.0] — 2026-06-16

### Added
- **es / fr / de translations** for the ~41 strings added across this cycle's new
  views (SLOs, interactive profiling, the reworked waterfall, RUM→trace, log
  histogram window, dashboard SLO/error widgets). English fallback already
  covered them; this restores full Spanish/French/German parity. ja/zh continue
  to fall back to English pending native sign-off, matching their existing state.

---

## [0.56.0] — 2026-06-15

### Changed
- Trace waterfall bars now show **self time** — a darker inner segment marking
  the portion of a span's duration not spent in its children, with a
  `total · self` tooltip. Makes it obvious at a glance whether a span is slow
  itself or just waiting on downstream work (standard APM read).

---

## [0.55.1] — 2026-06-15

### Changed
- Logs view gains a **time-window selector** (1h / 24h / 7d) driving the volume
  histogram and the level-count facets, so you can widen or zoom the volume
  context independently of the row limit.

---

## [0.55.0] — 2026-06-15

### Changed
- APM **Operations** table rows are now click-through: clicking an operation
  jumps to the traces list filtered to that service, mirroring the service-map
  edge pivot — so the latency/error table is a launch point into the traces
  behind a hot operation.

---

## [0.54.1] — 2026-06-15

### Documentation
- `docs/DEMO.md` updated to reflect the enriched `seed-demo`: the SLO + budget
  trend, RUM→trace and log↔trace links on the checkout path, the service-map
  edge metrics, the log-volume histogram, and the dashboard SLO/error widgets.

---

## [0.54.0] — 2026-06-15

### Added
- **Log-volume histogram.** The Logs view now shows an ELK-style volume
  histogram over the last 24h above the stream — one bar per time bucket with
  error-level (≥ error) volume stacked in red — honouring the active service /
  level / full-text filter. Backed by `GET /v1/logs/histogram` (`date_bin`
  bucketing, total + error counts).

---

## [0.53.0] — 2026-06-15

### Added
- **Dashboard recent-errors widget.** A sidebar panel lists the most recently
  seen open error issues across all projects (level dot, title, culprit, seen
  count), each linking to the issue. Backed by a new
  `GET /v1/error-issues/recent` cross-project feed. Sits beside the SLO widget,
  so monitor status, SLO budgets, and live errors share one glance.

---

## [0.52.0] — 2026-06-15

### Added
- **SLO achieved-ratio trend sparkline.** `/v1/slos` now returns a bucketed
  achieved-ratio `trend` per SLO (24 points over the window via `date_bin`),
  rendered as an auto-scaled inline sparkline next to each SLO's error-budget
  bar — so you see whether the budget is recovering or burning, not just the
  current number. Red when the SLO is breaching.

---

## [0.51.2] — 2026-06-15

### Changed
- `seed-demo` now populates the cross-tier links: the `/checkout` demo
  page-load carries the seeded checkout `trace_id` (RUM → trace), and the
  checkout-path demo logs are tagged with the same trace (log ↔ trace). The
  demo dashboard now demonstrates the correlation pivots end to end alongside
  the existing SLO seed.

---

## [0.51.1] — 2026-06-15

### Documentation
- New **`docs/CORRELATION.md`** maps the full cross-tier link web (log↔trace,
  error↔trace, trace→profiling by time window, RUM→trace, service-map edge→
  filtered traces) — how the ids flow and why single-tool correlation is the
  point. Linked from the docs nav and README.

---

## [0.51.0] — 2026-06-15

### Added
- **RUM → trace correlation.** RUM beacons can now carry the active backend
  `trace_id` (migration 0099 adds the nullable column); the browser snippet
  picks it up best-effort from `window.__rampartTraceId` or a
  `<meta name="traceparent">`. A new **Traced page-loads** table in the RUM view
  lists recent loads that carried a trace and deep-links each to its trace
  waterfall (`/v1/rum/traced`). Extends the cross-tier links (logs↔trace,
  error↔trace, trace↔profiling) to the browser tier.

---

## [0.50.0] — 2026-06-15

### Added
- **Dashboard SLO widget.** The dashboard sidebar now shows a compact SLO
  error-budget panel — a healthy/breaching marker plus the five worst budgets
  as bars (green → amber → red), each linking to the SLOs view. Hidden when no
  SLOs are defined. Surfaces budget burn next to monitor status at a glance.

---

## [0.49.1] — 2026-06-15

### Tested
- **Timed escalation climb** now has end-to-end integration coverage. New tests
  fast-forward the clock (backdating `next_escalation_at`) to prove a multi-rung
  ladder advances exactly one step per elapsed deadline with the next deadline
  reflecting the upcoming rung, that an acknowledge mid-climb halts it, and that
  a never-acked climb exhausts at the final rung. Closes the previously
  hand-verified-only gap; no behavior change. Docs note added to `ESCALATIONS.md`.

---

## [0.49.0] — 2026-06-15

### Changed
- **Service map** enriched from call-counts-only to a health view: each
  caller → callee edge now carries **error count / rate** and **p95 latency**
  of the callee span (one `percentile_cont` + filtered count added to the
  edge query). The map tab renders a throughput bar, a p95 pill, and an error
  pill (red past 1%) per edge, with service color swatches — and each edge is
  **clickable to jump to the traces list filtered to that callee service**.

---

## [0.48.0] — 2026-06-15

### Added
- **Trace → profiling time pivot.** Each span in the trace waterfall now offers
  *Profile this span's window* — it deep-links to the flamegraph scoped to that
  span's service and exact `[start, end]` interval. The `/v1/profiles/flamegraph`
  endpoint gained optional `from_ms` / `to_ms` (epoch-ms) parameters that take
  precedence over the rolling `hours` window (capped at 90 days); the Profiling
  view reads the window off the hash and shows a clearable "scoped to a span"
  banner. Connects the trace and profiling tiers by absolute time.

---

## [0.47.0] — 2026-06-15

### Changed
- **Profiling** flamegraph is now interactive, taking cues from Datadog / ELK /
  ScoutAPM: a **frame search** box dims non-matching frames and outlines the
  matches (with a match count); the **top-functions** table is clickable to
  highlight a function across the graph and shows a self-time bar; a **zoom
  breadcrumb** replaces the single reset button (click any ancestor to pop back);
  and a hovered-frame **info line** reports name, value, % of total, self time,
  and Δ in diff mode. Frame coloring and diff coloring are unchanged.

---

## [0.46.0] — 2026-06-15

### Changed
- Trace **waterfall** reworked into a proper call-tree timeline: subtrees now
  **collapse/expand** via a caret (with **Collapse all / Expand all**), rows are
  taller and evenly spaced with per-depth **indentation guides**, the time-axis
  ruler is **sticky** while the span list scrolls, a collapsed parent shows a
  `+N` hidden-children badge, and span attributes / status still expand inline
  on click. Per-service bar colors and the duration labels (placed at the end of
  each bar, flipping inside at the right edge) are retained.

---

## [0.45.0] — 2026-06-15

### Added
- **SLOs + error budgets** — a first-class tier (migration 0098). Define a named
  objective (e.g. 99.9% over 30 days) over one of two indicators: **monitor
  uptime** (up heartbeats / total) or a **metric ratio** (SUM good / SUM total
  over matching samples). The scheduler computes the achieved ratio and consumed
  error budget every tick and pages when the budget is **exhausted** or **fast
  burning** (Google-SRE 1-hour burn rate ≥ 14.4×), routing to channels and
  optionally climbing an **escalation policy** like the other rule kinds. New
  `/v1/slos` CRUD returns each SLO with a live snapshot (achieved %, budget
  remaining, 1h burn rate); new **SLOs** view renders the budget bars and an
  editor. `seed-demo` seeds an example metric SLO. Distinct from the existing
  per-monitor `slo_target_pct` marker, which stays as a simple uptime promise.

---

## [0.44.0] — 2026-06-15

### Changed
- **Navigation drawer** now groups collapse/expand. Each section has a chevron
  + item count; only the section holding the current view is open by default so
  the drawer stays compact as the product grows. Open/closed state persists per
  browser, an **Expand all / Collapse all** toggle sits in the header, filtering
  temporarily expands everything, and a collapsed section that contains the
  active view shows a dot marker.

---

## [0.43.0] — 2026-06-15

### Changed
- Trace **waterfall** redesigned to read as a proper timeline: a time-axis
  ruler with tick labels (0 → total) sits above the spans, quarter gridlines
  run through every track, bars are colored per service (with a matching swatch
  on the span label), and the duration label now sits at the end of each bar
  (flipping inside when the bar hugs the right edge). Call-tree indentation and
  click-to-expand span attributes / status message are unchanged.

---

## [0.42.0] — 2026-06-15

### Added
- Metric + detection alert rules can route through an **escalation policy**
  (migration 0097), like telemetry rules. Metric rules climb/resolve on their
  firing lifecycle; detection rules open an episode on a finding and auto-resolve
  when the rule goes quiet (no finding within ~2x its window). Detection rule
  form gains an escalation picker.


## [0.41.0] — 2026-06-15

### Changed
- Trace waterfall: now a **call-tree breakdown** — spans are nested by
  parent/child with indentation (DFS order) instead of a flat list. Click any
  span to **expand its attributes** (full `db.statement`/SQL, `http.*`, etc.,
  wrapped so long values are fully visible) and its error `status_message`.
  Data was already returned; this surfaces it.

---

## [0.40.0] — 2026-06-15

### Added
- **Timed escalation climb for alert rules.** Escalation episodes are now keyed
  on a generic subject (`subject_kind`/`subject_ref`, migration 0096; monitor
  episodes keep `monitor_id` for the FK cascade). A firing telemetry rule with
  an escalation policy opens an episode, pages step 0, and **climbs the ladder
  over time** (`check_escalations` branches monitor vs rule, checking the rule's
  firing state) until it recovers or is acked. New `GET
  /v1/escalation-policies/episodes` (all open) + `POST …/episodes/{id}/ack`
  (subject-agnostic ack); the Escalations page shows open episodes with an
  Acknowledge button. Replaces v0.39's on-fire fan-out with the real climb. The
  monitor ladder is unchanged.

---

## [0.39.0] — 2026-06-15

### Added
- Telemetry alert rules can route through an **escalation policy**
  (`escalation_policy_id`, migration 0095). On fire, the policy's full recipient
  set — every step's channels plus each schedule's current on-call (channel or
  user) — is paged in addition to the rule's own channels. The alert-rule form
  gains an escalation-policy picker. (This fans out to the whole ladder on fire;
  the timed per-step climb for rule subjects remains a follow-up — the episode
  engine is monitor+heartbeat-shaped end to end.)

---

## [0.38.0] — 2026-06-15

### Added
- Cross-tier correlation: **trace → errors**. Error ingest now extracts
  `contexts.trace.trace_id` into an indexed `error_events.trace_id` (migration
  0094, backfilled), `GET /v1/error-issues/by-trace/{trace_id}` lists the issues
  a trace touched, and the trace detail shows an "Errors in this trace" section
  linking to each issue (new `#/errors/<id>` deep link). Completes the
  Errors↔Traces↔Logs triangle (error→trace + log↔trace already existed).

---

## [0.37.0] — 2026-06-15

### Added
- On-call schedules can now rotate over **users**, not just notification
  channels. A user on call is paged at their account email; the rotation ring is
  channels followed by users (`participant_user_ids`, migration 0093). The
  schedule form gains a user picker. Escalation steps that reference a schedule
  page whoever — channel or user — is on call.

### Fixed
- `examples/demo-app`: `rampart` + `demo-backend` now `restart: unless-stopped`
  so a transient DB/DNS blip self-heals instead of leaving a dead container.

---

## [0.36.0] — 2026-06-15

### Added
- **`examples/demo-app`**: a standalone instrumented sample app (Node/Express
  backend + browser frontend, its own Postgres + Redis) wired to Rampart so
  every tier fills with real data — auto-instrumented traces (http/express/pg/
  redis), OTLP logs, periodic V8 CPU profiles (folded), RUM web-vitals + browser
  errors, backend 500s, and repeated "failed login" logs that trip a SIEM
  detection rule. `docker compose up --build`.

---

## [0.35.0] — 2026-06-15

### Added
- **Custom dashboards** (`#/dashboards`): build your own boards of widgets
  (monitor status, metric chart + sparkline, RUM web-vitals, notes). Per-user,
  saved in the prefs blob — no backend or schema. Reachable from the nav drawer
  (Overview).

---

## [0.34.0] — 2026-06-15

### Added
- **Prometheus `remote_write` ingest** (`POST /prom/write`): accepts the
  snappy-compressed protobuf `WriteRequest` and stores each series in the
  metrics tier, so Rampart is a metrics **sink**, not just a `/metrics` source.
  Hand-written prost subset + pure-Rust snappy (no new heavy deps); public with
  the optional shared-token gate. The full-stack example's Prometheus
  remote_writes into Rampart.

---

## [0.33.0] — 2026-06-15

### Fixed
- Navigation regression: the nav overhaul removed the dashboard header menu in
  favour of a floating launcher, which users couldn't find ("clicking
  Observability no longer works"). Restored a header ☰ button that opens the
  global nav drawer (any view can open it via a `rampart:nav-open` event).

### Added
- Self-RUM: `RAMPART_SELF_RUM=1` injects the RUM snippet into the dashboard
  shell at serve time, so the app reports its own Core Web Vitals + browser JS
  errors (real RUM + error data — same dogfood idea as the OTLP self-export).
  Same-origin, so the strict CSP already allows it. The full-stack example
  enables it.

---

## [0.32.0] — 2026-06-15

### Added
- **Self-observability**: with `RAMPART_OTLP_ENDPOINT` set, Rampart exports its
  own request traces + internal logs via OTLP/HTTP (pure-Rust, blocking reqwest;
  no grpc/C). Ingest + scrape routes (`/otlp`, `/rum`, `/api`, `/healthz`,
  `/metrics`) are excluded from span creation so pointing it at itself can't
  feed back into a loop.

### Changed
- Full-stack example now shows **real** data: Rampart self-exports its own
  traces + logs (endpoint → itself), and the load generator drives real API
  traffic instead of fabricating OTLP/RUM/error payloads.

---

## [0.31.2] — 2026-06-15

### Added
- `reset-password` now self-verifies: after setting the password it re-reads the
  row, verifies the hash in-process, and prints the target `DATABASE_URL` + the
  result. A green line proves the login works in *that* database — turning a
  recurring "still unauthorized" into a one-line diagnosis (typo vs wrong
  DB/instance).

---

## [0.31.1] — 2026-06-15

### Fixed
- Full-stack example: login "unauthorized" after `reset-password` was a stale
  cached `latest` image (binary predated the subcommand, so it booted a server
  instead of creating a user). `pull_policy: always` on the rampart services +
  a Troubleshooting section. Backend login verified end-to-end (200 + session).

---

## [0.31.0] — 2026-06-15

### Added
- `rampart-api reset-password <email> <password>` subcommand — break-glass admin
  recovery: resets the password if the user exists, else creates an admin.
  Server-side, so it bypasses the API password policy. No more raw psql.
- Demo runs can use **your own login**: `seed-demo` creates the admin from
  `RAMPART_ADMIN_EMAIL` / `RAMPART_ADMIN_PASSWORD` when set (and no user exists
  yet). The full-stack example threads these through compose + the load
  generator, so `RAMPART_ADMIN_EMAIL=… RAMPART_ADMIN_PASSWORD=… docker compose up`
  signs you in with your credentials.

---

## [0.30.1] — 2026-06-15

### Fixed
- Full-stack example: the load generator's demo admin password
  (`demo-password-123`) was rejected by the password policy (it contained the
  email local-part `demo`), so no admin was created and login failed. Use a
  compliant password and verify the session after register.

---

## [0.30.0] — 2026-06-15

### Added
- Full live example stack (`examples/full-stack/`): one `docker compose up`
  brings up Rampart + Postgres + the demo seed + a load generator (live OTLP
  traces/logs, RUM, errors) + healthy/flaky probe targets + Prometheus +
  Alertmanager wired back into Rampart's inbound webhook — the whole platform
  with data flowing live. `seed-demo` also seeds a status page + a fixed
  Alertmanager ingest token (new `ingest_tokens::create_with_token`) so the
  stack is turnkey.

### Changed
- Navigation overhaul: one consistent, theme-aware **global nav drawer** on
  every view — links grouped into Overview / Observability / Alerting / Catalog
  / Administration / Settings, role-filtered, with a filter box and active
  highlight. Replaces the old dev-only floating view switcher (which leaked into
  production and duplicated the dashboard's menus) and the redundant dashboard
  hamburger.

---

## [0.29.0] — 2026-06-15

### Added
- `rampart-api seed-demo` subcommand: fills a fresh instance with one
  representative slice of every tier (monitors + 48h uptime history, a folder, a
  notification channel, an error project with grouped issues + breadcrumbs, a
  multi-service trace, logs, RUM, a metric series, a telemetry alert rule, and a
  SIEM detection rule that raises a finding) so the whole dashboard lights up
  before any real telemetry. Idempotent; everything tagged `[demo]`. See
  docs/DEMO.md.

---

## [0.28.0] — 2026-06-15

### Added
- Ingest head-sampling: keep a configurable percentage of traces and logs at
  the OTLP ingest endpoints to cap storage. Deterministic, hashed on `trace_id`
  so a trace is kept or dropped whole (waterfalls stay intact) consistently
  across batches/replicas; trace-less logs use a per-record key. Settings →
  Ingest, plus `GET/PUT /v1/settings/ingest-sampling`. Default 100% (off);
  errors, metrics and uptime checks are never sampled.

---

## [0.27.0] — 2026-06-15

### Added
- Retention settings now expose **every tier** (heartbeats, uptime rollups,
  metrics, traces, logs, RUM, profiles, audit) instead of just heartbeats +
  audit. `GET/PUT /v1/settings/retention` round-trip the full config (effective
  values, defaults merged), each window validated 1–36500 days.

---

## [0.26.0] — 2026-06-15

### Added
- Storage footprint visibility: a per-tier on-disk size + estimated row-count
  panel on the Retention settings page, a `GET /v1/settings/storage` endpoint,
  and `rampart_table_bytes{table}` on `/metrics` — so operators can see what's
  growing and tune retention with data.

### Changed
- Large telemetry text/JSON columns (`logs.body`/`attributes`,
  `spans.attributes`, `error_events.context`/`stacktrace`) now use **lz4** TOAST
  compression instead of the pglz default (migration 0092; applies to rows
  written after upgrade; falls back to pglz on a Postgres built without lz4).

---

## [0.25.0] — 2026-06-15

### Added
- Error issue assignment: assign an issue to a user from the detail header
  (`POST /v1/error-issues/{id}/assign`), backed by an editor-visible assignee
  directory (`GET /v1/error-issues/assignable-users`). Uses the `assignee`
  column that already existed on `error_issues`.

---

## [0.24.0] — 2026-06-15

### Added
- Saved searches on the Logs and Traces views: name the current filter set and
  recall it as a chip. Per-user, stored in the existing prefs blob (a new
  `patchPrefs` merge keeps the dashboard's saved views untouched) — no new
  endpoint or schema.

---

## [0.23.0] — 2026-06-14

### Added
- Detection rules: optional **attribute-key match** (`attr_key` = `attr_val`
  against a log's JSONB `attributes`), so a rule can key off a structured field
  (e.g. `event.action = user.delete`) rather than a body regex. Migration 0091;
  honoured by evaluation + preview; editable on the rule form.

---

## [0.22.0] — 2026-06-14

### Added
- SIEM export now forwards **detection findings** alongside the audit log, over
  the same webhook/syslog sink (own `created_at` cursor, RFC5424 APP-NAME
  `detection`). The blue team's SIEM gets Rampart's findings, not just its audit
  trail.

---

## [0.21.0] — 2026-06-14

### Added
- Detection rule preview: `POST /v1/detection-rules/preview` and a **Preview**
  button on the rule form dry-run a match spec over recent logs (match count +
  sample lines) without saving — tune a pattern before enabling it.
- Alerting-pipeline & ingest metrics on `/metrics`: `rampart_metric_rules`,
  `rampart_metric_rules_firing`, `rampart_telemetry_rules`,
  `rampart_telemetry_rules_firing`, `rampart_detection_rules_enabled`,
  `rampart_detection_findings_open`, `rampart_escalations_open`,
  `rampart_error_issues_unresolved`, and `rampart_ingest_24h{tier}` — so an
  operator can alert on Rampart's own alerting and ingest health.

---

## [0.20.0] — 2026-06-14

### Added
- SIEM detection rules: occurrence-based rules over the log tier (service scope,
  OTLP severity floor, case-insensitive body regex) that raise a finding and
  notify channels when matches cross a threshold in a window. New
  `/v1/detection-rules` CRUD + findings feed/acknowledge API, a `#/detection`
  dashboard (Findings triage + Rules), migration 0090, and a restart-safe
  watermarked evaluation tick. See docs/design/DETECTION.md.

---

## [0.19.0] — 2026-06-14

### Added
- Error breadcrumbs: the SDK trail leading up to an error (kept verbatim in the
  event context) now renders as a category/message/level/time timeline on the
  issue detail.
- Logs CSV export: `GET /v1/logs/export.csv` and a download button on the Logs
  view, honouring the active service/level/search/trace filters (capped at 50k
  rows).
- Traces CSV export: `GET /v1/traces/export.csv` and a download button on the
  Traces view, honouring the active filters (capped at 50k rows).

### Changed
- Internal: CSV field escaping for all export endpoints (audit, delivery log,
  monitors, logs, traces) now lives in one shared `csv` module instead of being
  duplicated per route.

---

## [0.18.0] — 2026-06-14

### Added
- Error-issue statistics: distinct users affected plus breakdowns by release and
  environment, on a new `GET /v1/error-issues/{id}/stats` endpoint and an issue
  detail panel.
- RUM Core-Web-Vitals alerting: a `rum_lcp_p75` telemetry-rule kind that fires on
  the p75 of real-user Largest-Contentful-Paint (`lcp_ms`) over a rolling window,
  optionally scoped to an app.

---

## [0.17.0] — 2026-06-14

### Added

- **Alert silencing / mute.** Suppress notifications during a deploy or known
  noise — a silence is global or scoped to one monitor, with an optional expiry
  (`/v1/silences`, new **Silences** view). Enforced at the notifier's single
  dispatch chokepoint, so every alert path honours it (status flip, SLO,
  metric/telemetry rules, and the escalation ladder); a manual channel test
  always sends. Migration `0088`.

---

## [0.16.0] — 2026-06-14

### Added

- **Active-session management.** `GET /v1/sessions` lists your logged-in devices
  (IP / user-agent / age, current flagged); revoke one (`DELETE /v1/sessions/{id}`)
  or sign out all others (`POST /v1/sessions/revoke-others`). On the Security page.
- **2FA-enforcement policy.** Admins can require two-factor org-wide or for
  admins only (`settings.require_2fa` via `/v1/settings/security`); a user it
  applies to is forced to enrol before using the app.

### Security

- **Stronger password policy.** A shared validator now rejects too-common
  passwords, passwords containing the email name, and single-repeated-character
  passwords (on top of the length minimum) — applied to register, admin
  user-create, and password change.
- **Syslog-over-TCP SIEM sink** (`syslog_tcp`) for reliable streaming / a
  TLS-terminating sidecar, alongside UDP syslog and webhook. The `siem_export`
  config is now encrypted at rest with the other secret settings.

---

## [0.15.0] — 2026-06-14

### Added

- **HTTP monitor assertions.** Plain HTTP / keyword / json monitors can now carry
  a `config.assertions` array — status / header / JSON-path / body-substring
  checks with `eq`/`ne`/`lt`/`lte`/`gt`/`gte`/`contains` operators, the same
  engine the synthetic kind uses. Any failed assertion flips the monitor Down
  with the specific reason. Previously rich response checks lived only in the
  niche synthetic kind.
- **Trace search / filtering.** The traces list now filters by service, minimum
  duration, errors-only, and a substring on root operation / service / trace_id
  (`GET /v1/traces?service=&min_duration_ms=&errors_only=&q=`), with a filter bar
  in the UI. Previously the list was an unfiltered most-recent-100 — the #1 APM
  usability gap from the audit.

### Fixed

- **`max_retries` now works on its own.** Previously a monitor only retried a
  failed probe before flipping Down if its config also carried a `retry_backoff`
  block; `max_retries` alone did nothing. It now defaults to a fixed wait of
  `retry_interval_sec` between retries (0 = retry immediately), so "retry N times
  before alerting" behaves as the UI implies. *Behavior change:* monitors with
  `max_retries > 0` will now actually retry, slightly delaying down-detection.
- **`resend_interval_sec` now re-alerts.** It was dead config — a still-down
  monitor never re-paged unless an escalation policy was attached. The probe task
  now re-fires the down notification every `resend_interval_sec` while the
  monitor stays down (0 = off, the default), clearing on recovery.

### Security

- **Sessions are revoked on credential / role / 2FA change.** A password change,
  role/admin change, or 2FA-disable now deletes that user's sessions — a reset,
  demotion, or compromise can no longer leave a stale (still-privileged) session
  alive for its full 14-day TTL. A self-service password change re-issues the
  current device's cookie, so you stay signed in here while every other device
  is signed out. (`sessions::delete_for_user`, called from the user mutators.)

---

## [0.14.0] — 2026-06-14

### Added

- **Anomaly alerting (metric rules).** A new metric-rule op `anomaly` fires when
  the latest sample deviates more than `threshold` σ from the series' rolling 6h
  mean/stddev baseline — adaptive alerting for metrics whose "normal" drifts,
  where a static threshold would nag or miss. Reuses the metric-rule engine +
  sustain window + notifier; migration `0087` extends the op constraint. Flat
  series never alarm; a baseline needs ≥2 samples.
- **Monitors-as-code.** `GET /v1/monitors/export` dumps every monitor as a
  declarative spec (no ids/timestamps/runtime); `POST /v1/monitors/apply`
  reconciles a spec keyed by name — create new, update existing in place
  (keeping id + history), and `prune: true` deletes the unlisted. Per-item
  errors are collected; the run is one `monitors.apply` audit event. Keep the
  catalog in git. See [`docs/design/MONITORS-AS-CODE.md`](docs/design/MONITORS-AS-CODE.md).
- **SIEM / syslog export.** Stream the audit log (logins, failed logins, 2FA
  failures, config changes) to an external sink — **webhook** (HTTP POST a JSON
  array) or **syslog** (UDP, RFC5424 line per entry) — via a leader-gated
  forward tail with a persisted cursor (advances only on a successful send). Off
  by default; configure under Settings → Ingest or `PUT /v1/settings/siem-export`.
  Rampart isn't a SIEM — this feeds the one you have. See
  [`docs/design/SIEM.md`](docs/design/SIEM.md).

---

## [0.13.0] — 2026-06-14

### Added

- **Profile-type alerting.** A new telemetry-rule kind `profile_samples` fires
  when a service's profiling sample volume crosses a threshold over a window —
  e.g. CPU sampling spiking under load. Reuses the existing rule engine,
  notifier fan-out, and Alert-rules UI; migration `0085` extends the kind
  constraint.
- **Security insights.** The audit view's **Security** filter now shows an
  insights strip — failed / successful logins, 2FA failures, the top source IPs
  behind failed logins, and a per-hour failed-login sparkline over the last 24h
  (`GET /v1/audit-log/insights`). A security-event surface over the existing
  tamper-evident audit log — not an inline WAF.
- **Server-side symbolication (JS source maps).** Upload a build's source map
  per `(release, file)` on an error project; minified stack frames resolve to the
  original function / file / line on read (`GET /v1/error-issues/{id}/events`)
  via the `sourcemap` crate. The ingest parser now captures each frame's column
  (essential for fully-minified single-line bundles). Manage maps in the
  dashboard; migration `0086`. Native (DWARF) symbolication is deferred.

### Documentation

- **Tunneling stance** ([`docs/design/TUNNELING.md`](docs/design/TUNNELING.md)).
  Rampart deliberately ships no inline tunnel / proxy data plane; private-network
  reach is the probe agent's job (outbound-only, no inbound exposure). Documents
  the position, the agent answer, and the alternatives (WireGuard / Tailscale /
  cloudflared) for genuine tunneling.

---

## [0.12.0] — 2026-06-14

### Added

- **Profiling: trace→flamegraph correlation + diff flamegraphs.** A trace's
  detail now links its root service to that service's flamegraph
  (`#/profiling?service=…`), closing the loop with the APM tier. The Profiling
  view gains a **Diff** toggle (`GET /v1/profiles/flamegraph/diff`) that compares
  the current window against the preceding one and colors each frame by its
  after−before delta — red = hotter, blue = colder — so "what got slower since
  the deploy" reads off the colors.

---

## [0.11.0] — 2026-06-14

### Added

- **Continuous profiling tier — flamegraphs.** A fifth telemetry tier alongside
  errors / traces / logs / RUM. Push a profile in any of three formats —
  **pprof** (`POST /profiles/v1/pprof`; Go/Rust/py-spy/async-profiler/Pyroscope),
  **OTLP profiles** (`POST /otlp/v1development/profiles`, on the existing `/otlp`
  surface), or **folded text** (`POST /profiles/v1/folded`) — each lowered to a
  folded-stack map and stored (migration `0084`). The new **Profiling** view
  (`#/profiling`) renders an icicle **flamegraph** (click-to-zoom) plus a
  top-functions table (self vs total), merged over a service/type window
  (`GET /v1/profiles/flamegraph`); `GET /v1/profiles/{id}/flamegraph` shows one
  profile. Profiles age out via the prune loop (`profiles_days`, default 7).
  Ingest honors the optional telemetry token + IP rate limit. See
  [`docs/design/PROFILING.md`](docs/design/PROFILING.md).

---

## [0.10.0] — 2026-06-14

### Added

- **Logging depth — live tail + severity-volume breakdown.** The logs view gains
  a **Live** toggle (DB-backed polling, so it works across replicas) and a
  per-level **volume bar** (`GET /v1/logs/levels`) with click-to-filter chips.
  (Log→trace pivot already shipped — a log's `trace_id` links to the waterfall.)
- **APM depth — per-operation latency/throughput/error rollup.** The traces
  tier gains an **Operations** tab + `GET /v1/traces/operations`: spans grouped
  by `(service, operation)` with call volume, error rate, and p50/p95/p99/avg/
  max latency — the "services & resources" numbers (ScoutAPM/Datadog-style) on
  top of the existing trace list, waterfall and service map.

### Security

- **Authentication events audited.** Logins, failed logins and 2FA failures
  now land in the tamper-evident audit log: `auth.login` on success (password
  and TOTP paths), `auth.login_failed` on a bad password (recorded anonymously
  with source IP + attempted email — the brute-force / credential-stuffing
  signal), and `auth.totp_failed` on a correct password but wrong second factor.
  The audit view adds a one-click **Security** filter (scopes to `auth.*`) and a
  **Verify integrity** button that re-walks the hash chain (`GET /v1/audit-log/
  verify`) and flags a broken or deleted link inline — a security-event surface
  without a separate SIEM. Login attempts are bounded by the existing login rate
  limiter, so the chain can't be flooded.
- **Tamper-evident audit log.** Audit rows are now linked in a hash chain —
  each row stores `HMAC-SHA256(RAMPART_SECRET_KEY, prev_hash ‖ row)` (SHA-256
  fallback with no key). Because the MAC key lives outside the database, a party
  who can write to the DB can't edit, delete or reorder history without breaking
  the chain undetectably. `GET /v1/audit-log/verify` (admin) re-walks it and
  reports the first tampered row. Appends are serialized (advisory lock) so the
  chain stays linear; pre-existing rows are exempt (chain covers entries written
  after the upgrade).
- **SMTP + ingest secrets encrypted at rest.** Extends the at-rest encryption
  to the credential-bearing `settings` rows — the SMTP password and the
  telemetry ingest token are now sealed with the same AES-GCM envelope,
  transparently to readers (the notifier's SMTP loader, the ingest-token
  check). Closes the gap where channel configs were encrypted but these
  single secrets stayed plaintext.
- **SSRF guard extended to all connect-based probes.** A central guard in the
  probe dispatch now vets every protocol/DB/banner probe (Postgres, MySQL,
  MSSQL, Redis, Mongo, Memcached, Cassandra, NATS, LDAP, AMQP, MQTT, Kafka,
  gRPC, SNMP, RADIUS, NTP, WebSocket, TLS, SSH/SMTP/IMAP/FTP/POP3 banners,
  Steam) against the same loopback/link-local/metadata (+ opt-in private)
  blocklist — closing the gap where those kinds could reach internal hosts.
  HTTP/TCP/synthetic keep their own (address-pinning) guards; DNS/Domain/RDAP/
  DoH and hostless kinds are exempt by design.

---

## [0.9.0] — 2026-06-14

### Security

- **Notification secrets encrypted at rest.** Channel `config` blobs hold live
  credentials (webhook bearer tokens, SMTP passwords, the API keys of 130
  channels) and were stored as plaintext JSONB — a DB read leaked every
  outbound credential. They're now AES-256-GCM envelope-encrypted at the DB
  layer (`rampart_db::secrets`): sealed on write, transparently opened on read
  (so the notifier dispatch path is unaffected). Opt-in + backward compatible
  via `RAMPART_SECRET_KEY` (32-byte key, hex/base64) — key-less installs keep
  plaintext; setting a key encrypts lazily on next write while still reading
  old rows. The Helm chart **auto-generates + persists** a key, so K8s installs
  encrypt by default.
- **Ingest rate limiting + optional mandatory auth.** The public telemetry
  surfaces (`/otlp`, `/rum`, `/api` Sentry) now sit behind a per-IP token
  bucket (240 burst, 4/s refill) so a single source can't flood the tiers or
  fill the disk — legitimate collectors are unaffected. Setting
  `RAMPART_REQUIRE_INGEST_AUTH` makes ingest auth mandatory: an open
  (token-less) ingest surface is refused outright, forcing a configured token.
- **SSRF guard on outbound probes.** Probes that take a user-supplied target
  (HTTP/keyword/JSON, raw TCP, multi-step synthetics) now resolve the host and
  refuse to connect to loopback, link-local and the cloud **metadata IP
  (169.254.169.254)** + their IPv6 equivalents — always. Private/internal
  ranges (RFC1918, CGNAT 100.64/10, IPv6 ULA) are additionally blocked when
  `RAMPART_SSRF_BLOCK_PRIVATE` is set (opt-in: homelabs legitimately monitor
  private IPs; recommended on for multi-user / internet-exposed installs). The
  TCP probe pins the vetted addresses so a DNS rebind can't swap in a blocked
  IP after the check. Stops Rampart being used to reach cloud metadata or
  internal-only services via a crafted monitor.

### Added

- **OIDC / SSO login.** Rampart can sit behind your identity provider (Google,
  Okta, Keycloak, Authentik, Entra, …) instead of local password accounts —
  the #1 blocker for org adoption. Generic OpenID Connect via the Authorization
  Code flow with **PKCE**; identity is read from the provider's userinfo
  endpoint (server-to-server over TLS, so there's no JWT-signature handling to
  get wrong). Users are auto-provisioned by email on first login (the very
  first user bootstraps as admin; thereafter `RAMPART_OIDC_DEFAULT_ROLE`).
  Configured via env (`RAMPART_OIDC_ISSUER` / `CLIENT_ID` / `CLIENT_SECRET` /
  `REDIRECT_URL`); the login page shows a **Sign in with SSO** button when
  enabled. Routes under `/v1/auth/oidc`. Local password + 2FA still work.
- **Leader election for safe multi-replica / HA.** The scheduler, notifier
  digest-flush, escalation timers and retention prune now run only on the one
  replica holding a Postgres session **advisory lock** (`rampart_db::leader`).
  Previously every replica ran its own scheduler, so scaling past one pod
  meant N× probing and **duplicate alerts**; now extra replicas serve HTTP
  while a single leader owns the background work, and a follower takes over
  within ~10s of the leader exiting (active-passive HA + safe HorizontalPod
  Autoscaling — the Helm chart's autoscaling is now genuinely safe). Single
  replica is unchanged (lock acquired immediately).
- **Browser error capture (RUM → error tier).** The RUM snippet now hooks
  `window.onerror` + `unhandledrejection` and forwards uncaught front-end
  exceptions to `POST /rum/v1/errors`. The server records them in the
  error-tracking tier under a project auto-named after the beacon's app, so
  JavaScript errors group by fingerprint, appear in the Errors view, and fire
  the project's new/regressed alerts exactly like backend SDK errors. The raw
  stack is preserved in the event context (symbolication is a follow-up).
- **Full-text log search.** The logs view's body filter moved from an
  unindexed `ILIKE` substring scan to Postgres full-text search — a generated
  `body_tsv tsvector` column + GIN index (migration 0082), queried with
  `websearch_to_tsquery` so the search box understands bare words,
  `"quoted phrases"`, `or`, and `-exclude`.
- **Alerting on the observability tiers.** A new **telemetry alert rule** kind
  fires notifications when a rolling-window aggregate over the error, trace or
  log tier crosses a threshold — `error_rate` (error events/window),
  `trace_latency` (p95 span duration), `trace_error_rate` (% error spans) and
  `log_volume` (matching log count, with optional minimum severity and a body
  substring). Rules reuse the metric-rule state machine (the `for_seconds`
  sustain window, restart-safe fire/resolve dedup on persisted state) and page
  the same notification channels. Managed under **Alert rules** in the nav,
  evaluated on the scheduler tick alongside metric rules. New table
  `telemetry_alert_rules` (migration 0081); CRUD at `/v1/telemetry-rules`.
  (Error tracking already paged on new/regressed issues; this adds the
  rate/latency/volume dimension across all three tiers.)
- **Ingest compression + optional auth.** The OTLP trace/log endpoints
  (`/otlp/v1/traces`, `/otlp/v1/logs`) and the RUM beacon endpoint
  (`/rum/v1/events`) now transparently inflate `Content-Encoding: gzip` and
  `deflate` bodies — stock OpenTelemetry SDKs/Collectors gzip their OTLP/HTTP
  exports by default, so this is required for them to work out of the box. An
  optional shared **ingest token** (Settings → Ingest token, admin-only)
  guards these root-level surfaces: leave it blank to keep them open (the
  operator controls network exposure), or set it and have collectors present
  it via `Authorization: Bearer <token>` / `X-Rampart-Token`, or the RUM
  snippet's `data-token` attribute (`?k=` query param). The gzip/deflate
  helper is now shared with the Sentry error-ingest path.

### Fixed

- **Container image build** (`exit 101`). `routes/openapi.rs` `include_str!`s
  `docs/openapi.{yaml,json}` at compile time, but the Dockerfile only copied
  `backend/` — so the in-container release build couldn't read the spec. Copy
  the OpenAPI files into the build context. (Root cause was a missing file, not
  the memory pressure earlier suspected.)

---

## [0.8.0] — 2026-06-13

### Added

- **Cross-tier correlation** — the observability tiers now link to each other.
  A trace's detail view shows the **logs** emitted under that `trace_id`
  (and deep-links to the Logs view filtered to it); log lines link back to
  their trace; and an error issue whose event carries a trace context links
  straight to that **trace** and its **logs**. Traces + logs are now
  deep-linkable (`#/traces/{id}`, `#/logs/trace/{id}`).


- **Log ingestion — OTLP (Tier 3)** (migration `0079`) — ingest OpenTelemetry
  logs over OTLP and serve a filtered log stream. `POST /otlp/v1/logs` accepts
  an OTLP `ExportLogsServiceRequest` as both OTLP/JSON and OTLP/protobuf, so any
  OTel SDK/Collector logs exporter works by pointing at `http://<host>/otlp`.
  Each log stores its OTLP severity, message body, service, attributes, and the
  optional `trace_id`/`span_id` so logs correlate with the traces tier (table
  `logs`). Read API at `/v1/logs` filters by service, minimum level, body
  substring, or trace id; `/v1/logs/services` feeds the filter UI. A dashboard
  **Logs** view provides a service/level/search filter bar over a level-coloured
  stream with expandable per-line attributes + trace ids. Logs age out via a
  `logs_days` retention window (default 7) folded into the prune sweep. JSON log
  parsing is pure + unit-tested in rampart-core, reusing the trace tier's OTLP
  helpers. See [`docs/design/LOGS.md`](docs/design/LOGS.md). v1: ingest is
  network-scoped, uncompressed, unsampled; body search is substring (FTS is a
  follow-up).


- **Distributed tracing — OTLP (Tier 2 / APM)** (migration `0078`) — ingest
  OpenTelemetry spans and assemble them into traces. `POST /otlp/v1/traces`
  accepts an OTLP `ExportTraceServiceRequest` as **both** OTLP/JSON and
  OTLP/protobuf (content-type negotiated), so any OTel SDK or Collector works
  by pointing its OTLP/HTTP exporter at `http://<host>/otlp` — no
  Rampart-specific agent. A trace is the set of spans sharing a `trace_id`,
  assembled on read; spans store service, operation, kind, timing, status, and
  attributes (migration `0078`, table `spans`). Read API at `/v1/traces`:
  recent traces (root service/op, duration, span + error counts), a trace's
  spans for the waterfall, and a service dependency map from cross-service
  parent/child pairs. Dashboard **Traces** view with a trace list, a span
  waterfall, and a service-map tab. Spans age out via a `traces_days`
  retention window (default 7) folded into the prune sweep. JSON span parsing
  is pure + unit-tested in rampart-core. See
  [`docs/design/TRACES.md`](docs/design/TRACES.md). v1: ingest is
  network-scoped (unauthenticated), uncompressed, and unsampled — all
  follow-ups.
- **Error & exception tracking** (migration `0077`) — a self-hosted, Sentry-
  compatible error tier. Create an **error project** to mint a DSN; point any
  official Sentry SDK at it (`https://<key>@<host>/<id>`) and exceptions flow
  in — no Rampart-specific SDK. Ingest speaks the Sentry **envelope** and
  legacy **store** protocols (`POST /api/{id}/envelope/`, `/store/`), DSN-keyed
  and gzip/deflate-aware. Events are grouped into **issues** by fingerprint
  (explicit SDK fingerprint → exception type + in-app `(module, function)`
  frames with line numbers/paths dropped → normalized message), so a crash loop
  is one issue with a counter, not a flood. A resolved issue that recurs
  **reopens (regression)**. New + regressed issues alert through the existing
  notifier (new `error_new` / `error_regressed` event kinds) to the project's
  channels — off the ingest path, so the SDK response isn't blocked. Issues
  carry `unresolved`/`resolved`/`ignored` status; events age out per a
  per-project retention window (the issue persists). Admin API at
  `/v1/error-projects` + `/v1/error-issues`, a dashboard **Errors** view
  (projects → issues → stack-trace detail, with the DSN to copy), and event
  pruning folded into the retention sweep. Fingerprinting + Sentry-event
  parsing are pure + unit-tested in `rampart-core`. See
  [`docs/design/ERROR-TRACKING.md`](docs/design/ERROR-TRACKING.md). v1: no
  source-map de-minification, no auto cookie/breadcrumb UI, no post-create
  step editing of fingerprint rules.
- **Real User Monitoring (Tier 4)** (migration `0080`) — Core Web Vitals from
  real browsers, completing the observability platform. Rampart serves a tiny
  self-installing collector at `GET /rum/snippet.js`; one
  `<script src=".../rum/snippet.js" data-app="web">` tag collects LCP, INP, CLS,
  FCP, TTFB, and load time via PerformanceObserver + Navigation Timing and sends
  one beacon per page view on hide (`navigator.sendBeacon`, no dependency, no
  CORS preflight). Beacons land at `POST /rum/v1/events` (public; body parsed as
  JSON; useless beacons dropped). Read API at `/v1/rum`: `summary` (p75 per
  metric — the Web Vitals statistic — via `percentile_cont`), `pages` (per-URL
  rollup), and `apps`. A dashboard **RUM** view shows Web Vitals cards coloured
  good/needs-improvement/poor against the official thresholds, a per-page table,
  an app + window filter, and the copyable snippet. Events age out via a
  `rum_days` retention window (default 14). Beacon parsing is pure + unit-tested.
  See [`docs/design/RUM.md`](docs/design/RUM.md). v1: keyed by an app name (no
  per-app CRUD), INP approximated, no JS-error capture yet.
- **Synthetic transaction monitors** (migration `0076`) — a new `synthetic`
  monitor kind runs an ordered sequence of HTTP steps instead of one request.
  Each step makes a request, optionally extracts values from the response
  into named variables (`from`: a JSON path, a response header, or the status
  code), and asserts on the response (`status` / `json` / `header` /
  `body_contains` with `eq`/`ne`/`lt`/`lte`/`gt`/`gte`/`contains`). Variables
  interpolate into later steps via `{{name}}` — in the URL, header values, or
  body — so a "log in → capture token → call API → assert" flow works. The
  run stops at the first failed assertion and reports the failing step
  (`step 2 (login): json data.active == "true" (got "false")`); a clean sweep
  is Up with total wall-clock as latency. The step list lives in the existing
  `config.steps` JSONB (no schema churn); it rides the normal probe pipeline
  (retries, notifications, SLO, result webhooks). Wizard step-builder for
  creation; `timeout_seconds` applies per step. Zero new dependencies (the
  `{{var}}` interpolation and JSON-path lookup are hand-rolled). See
  [`docs/SYNTHETICS.md`](docs/SYNTHETICS.md). v1 carries no automatic cookie
  jar (carry session via extract → `{{var}}`) and no post-create step editing
  in the monitor modal yet.

---

## [0.7.0] — 2026-06-11

### Added

- **Escalation policies** (migration `0074`) — ordered notification
  ladders with acknowledge. A monitor referencing a policy routes its
  Down-flips through the ladder instead of the regular channel fan-out:
  step 1 pages immediately, each later step pages `wait_seconds` after
  the previous unless someone acknowledges
  (`POST /v1/monitors/{id}/escalation/ack`, button on the monitor page)
  or the monitor recovers — recovery notifies every step already paged.
  Escalation pages bypass digest coalescing and quiet hours by design.
  One episode per monitor is a database invariant (flap-proof);
  restart-safe state on the episode row; sends are delivery-logged with
  event kind `escalation`. Policy CRUD at `/v1/escalation-policies` +
  an Escalations dashboard view with a step builder, policy pickers in
  the wizard/edit modal, and an episode banner with Acknowledge. See
  [`docs/ESCALATIONS.md`](docs/ESCALATIONS.md).

- **Create monitor templates from scratch** — the Templates view grows
  a "New template" form (name, description, spec editor) alongside the
  existing "Save as template" capture path.

- **Host metrics via the probe agent** — `rampart-agent` now samples its
  host (CPU, memory, per-mount disk, load averages, uptime — `sysinfo`,
  no C build deps) every 60s (`RAMPART_AGENT_HOST_METRICS_SECS`, 0
  disables) and pushes through the new token-authed
  `POST /v1/agent/metrics`. The server injects an `agent="<name>"`
  label into every sample, so multi-host dashboards and per-host
  threshold rules work with zero configuration. Register an agent and
  its host appears in the Metrics explorer.

---

## [0.6.0] — 2026-06-11

### Added

- **Metrics: ingest, explore, alert** (migrations `0072`/`0073`) — push
  any metric to Rampart in Prometheus text format
  (`POST /v1/metrics/ingest`, Pushgateway-style, parsed by a
  zero-dependency parser; samples server-stamped) and read it back as
  series listings + bucketed range queries. **Threshold alert rules**
  watch one series each (`op`/`threshold` with a `for_seconds` sustain
  window) and page through explicitly-attached notification channels —
  channel templates, quiet-hour-free direct dispatch, and the delivery
  log all apply (`metric_rule_fired`/`metric_rule_resolved`). Restart-
  safe single-fire/single-resolve lifecycle persisted on the rule row;
  a series silent for 15 minutes resolves instead of alerting on stale
  data. A Metrics dashboard view (explorer with SVG charts + rules
  editor) and a `metrics_days` retention window (default 30). See
  [`docs/METRICS.md`](docs/METRICS.md).

- **Cron-job monitoring** (migration `0071`) — push monitors grow
  Cronitor-style run states: `/push/{token}/run` opens a duration clock
  (no heartbeat recorded), `/complete` closes it Up with the run's
  wall-clock duration recorded as the heartbeat latency (the
  response-time chart doubles as a run-duration chart), `/fail` records
  Down and notifies immediately (`?state=` works too; legacy
  `?status=up|down|warn` unchanged). Declaring `config.cron` (5-field
  UTC expression, parsed by a new zero-dependency parser in
  rampart-core) switches the monitor to schedule-aware mode: the
  scheduler synthesizes Down only for a **missed run**
  (`cron_grace_seconds`, default 300) or an **overrun**
  (`max_run_seconds`), and the healthy timeline belongs to the job's own
  pings — so a fail ping's Down is no longer overwritten by the next
  scheduler tick. Wizard fields for the schedule, copyable
  run/complete/fail URLs + a crontab example on the monitor page, and a
  read-only schedule card in the Config tab. See
  [`docs/CRON-JOBS.md`](docs/CRON-JOBS.md).

- **Remote probe agents** (migration `0070`) — run a lightweight
  `rampart-agent` worker in another region or a private network segment;
  it pulls its assigned monitors over the API, probes them locally with
  the same 38 probe runners the server uses, and reports heartbeats back
  in batches. The agent always dials out (NAT/firewall friendly, no
  inbound connectivity). Reported heartbeats ride the scheduler's writer
  pipeline, so notifications, SLO breach detection, result webhooks, and
  SSE streams behave identically to local probes; the local scheduler
  skips agent-assigned monitors. A stale-agent watchdog synthesizes a
  Down heartbeat (and pages) when an assigned monitor goes 2× its
  interval + 30s without a report. Admin `/v1/agents` CRUD mints a
  one-time `rmpa_…` bearer token (SHA-256 hash stored); the token-authed
  wire protocol is `GET /v1/agent/monitors` + `POST /v1/agent/heartbeats`.
  Dashboard: an Agents admin view (online badge, monitor counts, one-time
  token reveal) and a "Probe agent" picker in the monitor wizard + edit
  modal. Revoking an agent returns its monitors to local probing. See
  [`docs/AGENTS.md`](docs/AGENTS.md).

### Changed

- **Full ja/zh dashboard translations** — Japanese and Simplified Chinese
  locales now cover all 1081 keys (previously ~540 machine-draft keys with
  an English fallback for the rest), with terminology unified within each
  locale (ja: チャンネル/フォルダー; zh: 监控项/渠道). Still flagged
  pending human native-speaker sign-off in the file headers.

---

## [0.5.0] — 2026-06-10

### Added

#### Notifications

- **Per-channel quiet hours + rate limit** (migration `0060`) —
  `quiet_hours_start`/`quiet_hours_end` (UTC) suppress non-test sends
  inside the window; `rate_limit_per_hour` drops sends past a rolling
  1-hour cap. Channel-form fields in the Notifications view. Quiet hours
  and maintenance-window suppression verified to compose without
  double-handling — flips during maintenance are dropped upstream in the
  scheduler (never reach the notifier); maintenance start/end
  announcements respect a channel's quiet window like any other send.
  Pinned by tests.
- **Delivery log** (migration `0065`) — append-only `delivery_log` of
  every channel send attempt (success + failure, immediate + digest
  paths), recorded best-effort so a logging failure can never break
  dispatch. `GET /v1/delivery-log` (admin, keyset-paginated newest-first
  on the `before` cursor) + a read-only `#/delivery-log` view.
- **Outbound probe-result webhooks** — per-monitor `config.result_webhook`
  (JSONB) fire-and-forgets `{monitor_id, name, status, latency_ms,
  status_code, ts}` to a URL after every heartbeat (5s timeout, never
  blocks the scheduler). A "Result webhook URL" field (with http(s)
  validation) + an optional HMAC signing-secret field in the monitor
  wizard write it. When `config.result_webhook_secret` is set each POST
  carries `X-Rampart-Signature: sha256=<hmac>` over `<ts>.<body>` plus
  `X-Rampart-Timestamp` (replay-resistant); see
  [`docs/RESULT-WEBHOOKS.md`](docs/RESULT-WEBHOOKS.md) for the receiver
  guide + verification snippets. Each send (success/failure) is recorded
  in the delivery log.
- **Retry a failed delivery** — `POST /v1/delivery-log/{id}/retry`
  (admin) re-sends a logged delivery through its original channel,
  recording a fresh attempt; `409` when the channel was since deleted. A
  "Retry" button on failed delivery-log rows.
- **Scheduled uptime reports** (migration `0062`) — `scheduled_reports`
  table + `/v1/scheduled-reports` admin CRUD; a slow-tick renders
  per-monitor uptime and emails recipients via the SMTP path. Cadence is
  **daily / weekly / monthly** (lookback window matches), plus
  `POST /{id}/send` to send one out of band. A `#/reports` admin view
  manages them (list / create / edit / delete; name + recipients +
  cadence + send-now).
- **Digest-buffer restart test** — proves coalesced alerts persisted in
  `digest_buffer` (v0.4.0) are recovered + flushed after a restart.

#### API

- **Per-API-key rate limit + `X-RateLimit-*` headers** — a per-key
  hourly budget (default 1000/hr, configurable per key via
  `rate_limit_per_hour`, migration `0067`) enforced by a tower middleware
  layered inner to `require_session`. Over budget → `429` + `Retry-After`;
  under it → `X-RateLimit-Limit` / `-Remaining` / `-Reset`. Cookie/session
  requests are unlimited and get no headers. The counter is **durable
  across restart** (migration `0068`, `api_key_rate_usage` fixed-window
  table updated with a race-safe `INSERT … ON CONFLICT` per request);
  fail-open on a DB error.

#### Status pages

- **Component grouping / sections** (migration `0063`) —
  `status_page_sections` + `status_page_monitors.section_id`
  (`ON DELETE SET NULL`). The builder manages sections + per-monitor
  assignment; the public page renders monitors grouped under section
  headers (ungrouped first). `PublicStatusPage.sections` added.
- **Section reorder** — up/down controls on the sections panel persist
  new positions via the existing `updateSection` PATCH (accessible arrow
  buttons over fiddly HTML5 drag), optimistic local reorder then refetch.

#### Monitors

- **Header/cert presets → HTTP templates** (migration `0064`) —
  `monitor_presets` (named header / TLS bags) + `/v1/monitors/presets`
  CRUD + an "Apply preset" picker in the wizard. Presets extended to
  carry the whole HTTP config (method + accepted statuses + ignore-TLS,
  not just headers) so a preset prefills a near-complete monitor.
- **Bulk enable/disable by tag** — `POST /v1/monitors/bulk-by-tag
  {tag_id, action}` pauses/resumes every monitor carrying a tag; a
  tag-filter action on the dashboard.
- **Bulk-edit** — `POST /v1/monitors/bulk-edit {ids, patch:{interval_secs,
  timeout_secs, enabled, group_id, tags}}` applies a patch to up to 500
  monitors in one transaction (`tags` replaces the set, `group_id:null`
  clears the folder), returning `{updated, skipped}`. `?dry_run=true`
  returns a per-monitor field-level diff without mutating; a real edit
  returns a ready-to-replay `undo` payload. Dashboard multi-select bar
  with Preview + Undo.
- **Drag monitors between folders** — drag a monitor row onto a folder
  header (or Ungrouped) on the dashboard to reassign its folder via
  `PATCH group_id`; the existing folder UI + keyboard paths stay.
- **Monitor templates** (migration `0069`) — `monitor_templates`
  (named whole-monitor `spec` JSONB) + `/v1/monitor-templates` CRUD +
  `POST /{id}/instantiate` (optional name override) to spin up a new
  monitor. A `#/templates` library + a "Save as template" action on the
  monitor detail.
- **Hourly rollups + long-range uptime** (migration `0066`) — retention
  now **tiers** instead of flat-deleting: heartbeats older than the raw
  tier are downsampled into `heartbeat_rollups` (hourly up/down/other +
  avg latency, idempotent UPSERT) then the raw rows are dropped; rollups
  are kept ~1y. `GET /{id}/rollups` exposes the buckets and
  `GET /{id}/uptime-history?range=30d|90d|1y` returns a daily uptime
  series stitched from raw + rollups (works past the raw horizon). A
  monitor-detail uptime-history chart with a range selector.
- **Dependency-graph view** — a read-only `#/dependencies` page
  rendering the monitor dependency edges as a hand-rolled SVG graph
  (status-coloured nodes, click to open a monitor). No new backend.

#### Audit

- **Delivery-log CSV export** — `GET /v1/delivery-log/export.csv`
  (admin) streams the delivery log (sent_at, channel, event, monitor,
  ok, error); an "Export CSV" button on the view.

### Security

- **Custom-CSS sanitizer reassembly bypass** — `sanitizeCustomCss`
  stripped `</style`/`<script` in a single pass, so an interleaved
  payload like `<scr<scriptipt>` rejoined into a surviving `<script`
  after one removal. Now strips to a fixpoint (loop until stable),
  closing the `<style>`-breakout vector. Resolves CodeQL
  `js/incomplete-multi-character-sanitization`.

#### Tests + docs

- **7 new e2e specs** (generic webhook, per-incident RSS, incident
  templates, status-page sections, monitor presets + bulk-by-tag,
  scheduled reports, clone-to-folder) → 61 flows × 5 = 305 cross-browser
  runs.
- Upstream TLS/crypto blocks (Cassandra-TLS, rumqttc 0.25,
  rustls-webpki advisories) re-verified — no movement; dated note in
  `docs/SECURITY-DEBT.md`.

#### Internationalization

- **ja / zh coverage** — best-effort Japanese + Simplified-Chinese
  translations for every new surface (scheduled reports, delivery log,
  sections, presets, result-webhook + secret, bulk-edit preview/undo,
  uptime history, monitor templates, CSV export, folder drag) that would
  otherwise fall back to English through the `...en` spread. Both stay
  MACHINE-DRAFT pending native-speaker review.

### Notes

- Migrations `0060`/`0062`–`0069`, all additive. Backend features landed
  as per-concern commits; the OpenAPI drift guard caught and got every
  new route documented. The retention prune loop now downsamples to
  `heartbeat_rollups` before deleting — long history survives at hourly
  granularity past the raw tier.

---

## [0.4.0] — 2026-06-09

The "API-grade" release — 15 commits since v0.3.0. Locks down API-key
access with real scopes, machine-describes the REST surface with a
drift-guarded OpenAPI spec, makes alert ingestion fully generic,
hardens the notify path, and adds retry-backoff + incident templates +
dashboard view sharing.

### Added

#### Access + API surface

- **Per-API-key scope enforcement.** Keys now carry a `scope`
  (`read` / `write` / `admin`, migration `0057`) that maps onto the RBAC
  roles — a request authenticated by an API key gets that scope's
  effective role, so the existing route guards 403 a `read` key on
  mutations and a non-`admin` key on admin routes. Existing keys
  backfilled to `admin` (no silent downgrade of live keys). Scope picker
  + pill in the ApiKeys view.
- **OpenAPI 3.1 spec + drift guard.** Hand-curated `docs/openapi.yaml`
  served at `GET /openapi.yaml` + `/openapi.json`; a CI check
  (`scripts/check_openapi.py`) diffs every registered route against the
  spec and fails when one is undocumented (114 routes = 114 documented).
- **Subscriber self-manage page** at `#/manage/{token}` (no login) —
  list subscriptions, unsubscribe per-page or all.

#### Ingestion + notifications

- **Generic JSON-path webhook receiver** at
  `/v1/public/ingest/generic/{token}` — maps an arbitrary inbound JSON
  body to an incident via operator-configured RFC-6901 pointers
  (migration `0056`), beyond the 5 named vendors.
- **Per-incident Atom/RSS feeds** —
  `/v1/public/status-pages/{slug}/incidents/{id}/feed.{atom,rss}` for a
  single incident's update thread, linked from each incident card.
- **Digest buffer persisted to the DB** (migration `0055`) — coalesced
  flapping alerts now survive a restart instead of living only in
  process memory.

#### Monitors + status pages

- **Per-monitor retry backoff** — optional `config.retry_backoff`
  (`fixed` / `linear` / `exponential`, base + cap) sleeps between retry
  attempts; default behaviour unchanged. Wizard control + 6 unit tests.
- **Incident-template library** (migration `0059`) — reusable canned
  incident updates ("Investigating / Identified / Monitoring /
  Resolved") with a "Use template" picker + manage panel in the
  status-page builder.
- **"Maintenance now" quick action** — one-click 1h/4h/24h maintenance
  window on the monitor detail page.
- **HTTP protocol-version assertion** — optional
  `config.expected_http_version` fails the HTTP probe when the
  negotiated version (1.1/2/3) doesn't match.
- **Clone a monitor into a chosen folder/group** (target-group picker on
  the clone action).

#### Dashboard

- **Saved views + per-user preferences** (`users.prefs` JSONB, migration
  `0054`; `GET`/`PUT /v1/me/prefs`) — save/apply/delete named
  tag+folder+search filter combos + a default folder.
- **Shareable saved views** — export a view as a base64 token /
  `#/?view=<b64>` deep link, import to apply.

### Migrations

- `0054`–`0059` (user prefs, digest buffer, ingest-token mapping,
  api-key scope, incident dedup carried from 0.3, incident templates).
  All additive.

### Notes

- 15 commits since `v0.3.0` (27fa937). No breaking API changes. Existing
  API keys are backfilled to `admin` scope so nothing they could do
  before breaks; operators should re-scope keys down to least-privilege.

---

## [0.3.0] — 2026-06-09

The "operate it like a SaaS" release — 53 commits since v0.2.0. Adds
role-based access control, doubles the importer catalog, turns the
status page into a brandable multi-tenant surface, ships inbound
alert ingestion from five vendors, a full SLO suite, internationalised
the entire UI, and hardens CI.

### Added

#### Access control

- **RBAC — admin / editor / readonly.** Replaces the binary `is_admin`
  flag (kept one release as a rollback shim) with a `role` enum
  (migration `0048`). `editor` gets all monitor/incident/maintenance/
  status-page/notification CRUD; `readonly` is GET-only everywhere;
  `admin` keeps users/settings/security/api-keys/proxies/audit. Enforced
  by `require_admin` + a method-based `require_write_or_readonly_get`
  middleware; the frontend mirrors the classification (hidden admin nav
  + `canWrite`-gated write buttons) and the Users view gains a role
  picker. Backed by unit tests + a cross-browser RBAC e2e spec.

#### Importers (catalog 7 → 18 + generic CSV)

- **11 new SaaS importers** under `rampart-import`: Cachet, Gatus,
  Uptime.com, HetrixTools, Freshping, Checkly, StatusGator, Pingometer
  (+ BetterStack, Healthchecks.io, Cronitor, StatusCake, RapidSpike,
  Updown.io already counted). Each: a `parse_and_map` module, a fixture,
  two integration tests, and a docs/IMPORTERS.md section.
- **Generic CSV import** — both a `rampart-import csv <file>` CLI path
  and an in-app **upload widget** (`POST /v1/monitors/import-csv` +
  `#/import` view with a client-side validation preview table).

#### Inbound alert ingestion

- **Token-authed webhook receivers for 5 vendors** — Alertmanager,
  Grafana, Datadog, PagerDuty, Opsgenie — at
  `/v1/public/ingest/{vendor}/{token}`. Firing alerts open status-page
  incidents, resolved alerts close them, **fingerprint-deduplicated**
  (migration `0049`) so a repeated firing can't stack duplicates. Token
  management lives in the status-page builder (lists all 5 vendor URLs
  per token). Documented in `docs/INGEST.md`.

#### SLO + reliability suite

- **Per-monitor SLO targets** (`slo_target_pct` / `slo_window_days`,
  migration `0044`) with an Overview SLO card.
- **SLO breach + recovery notifications** (`EventKind::SloBreached` /
  `SloRecovered`, single-column dedup, migration `0045`).
- **Error budget** point-in-time fuel gauge + **burn-down chart** with a
  7/30/90-day window picker.
- **MTBF / MTTR** widget with a 7/30/90-day window picker.

#### Status page — brandable, private, observable

- **Custom domain** with Host-header routing + **logo upload** +
  **per-page custom CSS** (sanitised) + **password-protected private
  pages** (Argon2 + `/unlock`).
- **90-day daily uptime strip** with a click-day drilldown popover (per-
  hour latency mini-chart), **12-month uptime summary chips**, a **hero
  status banner**, a **KPI row**, **incident history**, a **scheduled-
  maintenance banner** with a live countdown, and **Atom + RSS feeds**.
- **TLS guidance** (Caddy / certbot) + **cert-manager Helm support** for
  custom domains.

#### Notifications

- **Maintenance start/end notifications** to attached channels + status-
  page email subscribers (migration `0050`).
- **Per-channel digest window** — coalesce flapping alerts into one
  message every N seconds (migration `0053`).
- **Per-monitor "test all attached channels"** action.

#### Observability + ops

- **`/metrics`** HTTP request counter + latency histogram (from v0.2 era,
  extended).
- **Audit-log streaming CSV export** (keyset-paginated, time-range
  filtered).
- **1-year heartbeat retention** default (migration `0043`).
- **Helm chart** at `charts/rampart/` (Deployment + Service + Ingress +
  cert-manager + optional embedded Postgres).

#### Internationalisation

- **Full UI i18n** — an in-house `t()` runtime + 6 locales (en source;
  es/fr/de full; ja/zh machine-draft pending native review) wired
  through every admin view + the public status page, with a floating
  locale picker.

#### Probes

- **AMQPS** (RabbitMQ over TLS) + confirmed **NATS-over-TLS**.

### Fixed

- Monitor edit modal, Notifications channels layout, global theme toggle,
  dashboard dead buttons, wizard Back button, dark-mode polish on the
  newer cards, and clear-via-`null` on `UpdateStatusPage` triple-state
  fields.

### CI / tooling

- Pre-commit hook (gitleaks + trufflehog + shellcheck + `cargo fmt`).
- Fixed the rust-embed `frontend/dist` build failure in the backend +
  e2e CI jobs.
- E2e matrix grew to **52 flows × 5 browsers**.

### Migrations

- `0043`–`0053` (retention, SLO, SLO-dedup, status-page branding,
  ingest tokens, user roles, incident dedup, maintenance-notified,
  status-page password, custom CSS, channel digest).

### Notes

- 53 commits since `v0.2.0` (5fa9c1d). No breaking API changes; all
  migrations are additive + idempotent. `is_admin` retained one release
  as an RBAC rollback shim.

---

## [0.2.0] — 2026-06-09

The "production-grade" release. Hardens the security surface, doubles the
importer catalog, rebuilds the public status page, ships in-process
HTTP metrics + 1-year heartbeat retention, and lands the operator
deployment artefacts (systemd unit + rotating pg backup script + hardened
compose).

### Added

#### Public status page — full visual redesign

- **Per-component 90-day daily uptime strip.** One coloured cell per day, oldest-left / today-right, with `u`/`d`/`w`/`m`/`n` encoding (operational / down / degraded / maintenance / no-data). Hover title reads "Operational · 14 days ago" etc. Backend `daily_status(monitor, 90)` does a single `GROUP BY date_trunc('day', …)` then pivots into a dense `Vec<u8>`.
- **12-month uptime summary chip row** under each strip — "Jun 99.97% · Jul 100% · …" with SLA-threshold colouring (≥99.9 green / ≥99 amber / <99 red / no-data grey). Backend `monthly_uptime(monitor, 12)` mirrors the daily helper.
- **Default heartbeat retention bumped 90 → 365 days** (migration `0043_retention_one_year.sql`) so the 12-month summary has real data. Idempotent: operator-tuned values preserved.
- **Hero status banner** — big "All Systems Operational" / "Active Incident" / "Service Disruption" / "Scheduled Maintenance" card with one-line subtitle + icon tile.
- **KPI row** — Components count, 90d uptime average, active-incidents count, last-update relative time. Mobile collapses 4-col → 2-col.
- **Active incidents section moved below components**, so the components signal leads. Pinning ranks within the section only — no longer floats above components.
- **Incident history** — last 30 resolved incidents with style-coloured left bar, "Resolved" pill, lasted-duration line ("47 minutes" / "2h 14m" / "3 days").
- **Atom 1.0 + RSS 2.0 feeds** at `/v1/public/status-pages/<slug>/feed.{atom,rss}`. Hand-rolled XML, RFC 3339 + RFC 822 timestamps, proper escape on user-controlled fields. Drops into Feedly / NetNewsWire / Reeder / Slack `/feed`.
- **Tabbed subscribe popover** — Email / RSS / Atom / Webhook. Email uses existing single-opt-in flow; RSS + Atom tabs show the feed URL with Copy + Open buttons; Webhook tab explains operator-side wiring.
- **Live auto-refresh indicator** with pulsing green dot in the brand row.
- **Footer legend** explaining the five strip colours.
- **Admin incident edit** now exposes the "Pin to top" toggle that was previously create-only.

#### Importers (3 new on top of the existing Site24x7)

- **Datadog Synthetics importer.** `rampart-import datadog <export.json>` maps the `(type, subtype)` shape Datadog's `/api/v1/synthetics/tests` returns onto Rampart monitor kinds — covers the `(api, http|tcp|dns|ssl|icmp|grpc|websocket|udp)` cluster plus `(browser, *)`; multi-step synthetics skip with a warn. `body+contains` assertions promote `(api, http)` to `Keyword`.
- **UptimeRobot importer.** `rampart-import uptimerobot <export.json>` maps UptimeRobot's integer-type schema (type 1-5 + sub_type 1-6/99 for port checks). Type 1 → Http, 2 → Keyword (keyword_value carries the substring), 3 → Ping, 4 → Tcp/Ftp/Smtp/Pop3/Imap by sub_type, 5 → Push.
- **Pingdom importer.** `rampart-import pingdom <export.json>` maps Pingdom's string-typed checks (`http`/`tcp`/`dns`/`ping`/`udp`/`pop3`/`smtp`/`imap`/`ssh`). HTTP checks reconstruct the full URL from Pingdom's split `hostname`+`port`+`encryption`+`url` fields. `should_contain` promotes to `Keyword`. `verify_certificate=false` translates to `ignore_tls=true`. Multi-step `transaction` checks skip.
- **Site24x7 importer** — `rampart-import site24x7 <export.json>` CLI (new `[[bin]]` in `rampart-api`) maps a Site24x7 `GET /api/monitors` JSON dump onto Rampart monitors via the existing repository layer. `--dry-run` prints a per-kind breakdown without touching the DB; `--skip-existing` makes the run idempotent on re-import. Documented in `docs/IMPORTERS.md`.

#### Security hardening

- **Security response headers + 2 MiB request body cap.** Every response now ships HSTS / X-Content-Type-Options nosniff / X-Frame-Options SAMEORIGIN / Referrer-Policy / Permissions-Policy (with browser-APIs Rampart doesn't use locked out) / Content-Security-Policy ('self' default with the minimum allow-list for the dashboard's actual remote sources). `RequestBodyLimitLayer(2 MiB)` rejects oversized bodies with 413 (or a clean connection close) before any handler sees them.
- **Per-IP rate limiter on the auth surface.** Token-bucket (capacity 10, refill 1 token / 6 seconds) on `/v1/auth/{register,login,logout,me}` + `/v1/auth/2fa/verify` caps brute-force at ~10 attempts/min per IP without locking out a user who fumbles their password. Opportunistic GC keeps the in-process IP map bounded; X-Forwarded-For + X-Real-IP both honoured.
- **Audit log payload redaction.** `rampart_api::audit::record` walks the JSON payload before persistence and replaces the value of any object key matching a known secret-pattern (`password`, `secret`, `token`, `api_key`, `private_key`, `client_secret`, `auth`, `credential`, `totp`, `recovery`, `vapid`, `smtp_password`, `bind_password`) with `"[redacted]"`.

#### Observability

- **`rampart_http_requests_total` + `rampart_http_request_duration_seconds` histogram on `/metrics`.** In-process middleware records every served request's method + status-class + handler latency. Buckets cover 1 ms → 10 s. Zero new external deps.

#### Operator deployment artefacts

- **Deployment artefacts** under `docs/deploy/`: `rampart.service` (systemd unit wrapping `docker compose up -d --wait` + healthcheck-gated startup), `backup-postgres.sh` (rotating `pg_dump --format=custom` via `docker exec`, 14-day default retention), `README.md` (install + restore + reverse-proxy + non-goals).
- **`compose.yaml` hardened.** Resource limits (Rampart 512 MiB / 1 CPU; Postgres 1 GiB / 1 CPU), `read_only: true` rootfs on Rampart with `tmpfs:/tmp`, `cap_drop: ALL` + `cap_add: NET_RAW`, `security_opt: no-new-privileges:true`, `/readyz`-based healthcheck, `RAMPART_LOG_FORMAT=json` default, `stop_grace_period: 30s`.

#### Tests + repo hygiene

- **8 new e2e specs.** `security-headers.spec.js` (6 specs locking in response-header + 413 body-cap + 429 rate-limit) + `api-keys.spec.js` + `proxies.spec.js`. E2E matrix is now 30 × 5 = 150 cross-browser runs per CI push.
- **Pre-commit hook.** `.pre-commit-config.yaml` at the repo root: pre-commit-hooks hygiene + gitleaks + trufflehog + shellcheck + a local `cargo fmt --all -- --check` hook.

#### UI polish

- **Monitor edit modal redesigned.** Added the missing `.input` CSS class (it was referenced but never defined in `MonitorDetail.jsx`'s style block, so every input fell through to browser defaults); rebuilt the modal as a sectioned form (Basics / Target / HTTP request / Behaviour / Probe config) with sticky header + footer, larger textareas, accent focus rings, and accent-soft toggle pills.
- **Global theme toggle.** Extracted `ThemeToggle` into `src/components/ThemeToggle.jsx` with both inline + floating variants. Floating variant mounted in `App.jsx` so every view (not just the dashboard) carries the light / dark / system cycle button.
- **Notifications channels page tightened.** Form panel constrained to max-width 720, channel-row icons rendered as accent-soft tiles, type/status caption replaced with pill + status-dot.
- **WebSocket probe doc-comment clarifies `wss://`** — the probe already accepted TLS URIs via the workspace-shared ring `CryptoProvider`; the comment + a URL-parser unit test make that explicit so a future refactor that breaks scheme handling fails CI loudly.

### Notes

- 19 commits since `v0.1.0` (29642a2).
- Default heartbeat retention change is migration-driven and idempotent. Installs that operator-tuned the value keep their choice; installs still on the 90-day seed get auto-promoted to 365.

## [0.1.0] — 2026-06-09

First public release. Single Rust binary backed by Postgres, ships a React dashboard embedded into the binary at compile time, covers 38 probe kinds and 130 notification channels.

### Core

- **38 probe kinds** — HTTP, Keyword (substring), JsonQuery, TCP, Ping (ICMP), DNS (A/AAAA/CNAME/MX/TXT/NS/SRV/CAA/SOA), Push (heartbeat-receive), gRPC `health.v1`, TLS handshake + cert expiry, Docker container, Steam (A2S), MQTT, RADIUS, Kafka (ApiVersions handshake), Postgres, MySQL, MSSQL, Redis, MongoDB, Memcached, NTP (SNTPv4), WebSocket (RFC 6455), NATS, LDAP, AMQP (RabbitMQ-compatible), DNS-over-HTTPS (RFC 8484 JSON variant), RDAP (RFC 7480/9082), SNMP v2c GET, Cassandra/ScyllaDB, mDNS service discovery, SSDP/UPnP, Domain expiry (WHOIS), headless-browser, SSH banner, SMTP banner, IMAP banner, FTP banner, POP3 banner.
- **130 notification channels** — Slack, Discord, Telegram, Microsoft Teams, Webhook (HMAC-signed), Email (generic SMTP + SendGrid/Resend/Brevo/Mailgun/Mailjet/Postmark/Mandrill/SparkPost), PagerDuty, Opsgenie, Pushover, Twilio + 26 other SMS providers, Signal, Matrix, Mattermost, Rocket.Chat, Gotify, ntfy, Pushy, Apprise gateway, Splunk On-Call + 18 other incident/on-call platforms, Web Push (RFC 8291 with VAPID), Mastodon/Nostr, Sentry/Rollbar/Honeybadger, AWS SNS / Azure Service Bus / GCP Pub/Sub, Home Assistant, plus 70+ other native adapters in `rampart-notifier/src/channels/`.
- **Scheduler** runs probes off a tokio-based dispatcher with per-monitor concurrency caps and jittered cadence; batched heartbeat writer cuts INSERT round-trips ~100×.
- **Liquid templates** for notification subject + body (`{{ … | filter }}` syntax, conditionals, loops). Preview button on the template editor renders against a fake heartbeat so you can iterate without a probe firing.
- **Folders + tags** for organising large fleets, with routing rules per folder.
- **Dependency-aware alerts** — mark monitor B as depending on monitor A and B suppresses its own outage notification when A is down. Cycle-guarded.
- **Maintenance windows** — schedule one-shot or recurring quiet periods per monitor / folder.
- **Status pages** — public, custom-domainable, dependency-aware. SSE stream means the visitor view updates without reload. Email subscriber sign-up with single-opt-in.
- **Audit log** of every state-changing action; filterable by `kind` / `action` / `actor` / `from` / `to` (RFC 3339 timestamps); CSV export honours the same filters.
- **2FA (TOTP)** with QR enrolment, recovery codes, and a sign-in challenge gate.
- **Push monitor** with regenerable token per monitor for incoming heartbeats from cron jobs.
- **Test-now button** to fire a probe out-of-cycle from the monitor detail view.
- **Bulk actions** on the dashboard (pause/resume/delete/move/attach-channel/detach-channel).
- **Per-monitor heartbeat CSV** export via `/v1/monitors/{id}/heartbeats.csv`.

### API

- `/v1/monitors`, `/v1/monitors/{id}/heartbeats` (cursor pagination via `?before=<rfc3339>&limit=<n>`), `/v1/monitors/{id}/test-now`.
- `/v1/notifications` (channels + counts), `/v1/notifications/{id}/test` (send a probe message through the real provider).
- `/v1/monitors/bulk` — multi-monitor action endpoint.
- `/v1/status-pages` (admin CRUD) + `/v1/public/status-pages/{slug}` (visitor surface) + `/v1/public/status-pages/{slug}/subscribe`.
- `/v1/maintenance-windows`, `/v1/monitor-groups` (folders), `/v1/monitors/{id}/dependencies/{parent_id}`, `/v1/tags`.
- `/v1/audit-log` + `/v1/audit-log/csv` with `kind` / `action` prefix / `actor` UUID / `from` / `to` filters.
- `/v1/auth/...` — register, login, logout, `/me`, TOTP enrol / verify / disable.
- `/v1/api-keys`, `/v1/proxies` (outbound HTTP proxy registry, selectable per-monitor), `/v1/users` (admin).
- `/v1/settings/smtp`, `/v1/settings/retention` (heartbeat + audit retention in days).
- `/healthz` (returns `{"status":"alive","version":"<x.y.z>"}`), `/readyz`, `/metrics` (Prometheus text — `rampart_build_info`, `rampart_monitors{status}`, `rampart_monitors_by_kind{kind}`, `rampart_channels_active`, `rampart_webpush_subscribers`, `rampart_heartbeats_24h{status}`, `rampart_incidents_open`).
- `/v1/stream/heartbeats` — Server-Sent Events live stream.
- `/push/{token}` — public push-monitor heartbeat sink (the token is the auth).
- Hand-curated endpoint reference in [`docs/API.md`](docs/API.md) catalogues every route with source-file pointers.

### Architecture

- Six Rust crates: `rampart-core` (types), `rampart-db` (sqlx repository), `rampart-checker` (probe runners — one file per probe), `rampart-scheduler` (dispatcher), `rampart-notifier` (channel adapters + Liquid template renderer), `rampart-api` (axum HTTP + the main bin).
- Single binary — the frontend bundle is embedded via `rust-embed`. Debug builds read from `frontend/dist`; release builds compile the bytes in.
- Workspace version centralised in `[workspace.package].version`; member crates inherit via `version.workspace = true`. `/healthz`, the Prometheus `rampart_build_info` gauge, the dashboard header pill, the HTTP probe User-Agent, and the Honeybadger notifier payload all read `env!("CARGO_PKG_VERSION")` — single decision point per release.
- sqlx **offline mode** with a committed `.sqlx/` cache. CI builds without bringing up Postgres for compile checks.
- Migrations live in `backend/migrations/000N_*.sql` and run on boot.
- MSRV is **Rust 1.88** (forced by transitive `time` 0.3.47, `base64ct` edition2024). The release Dockerfile uses `rust:1.96-slim-bookworm` for build speed + diagnostics; the MSRV floor is declared in `[workspace.package].rust-version` because that's the contract with library consumers and downstream packagers, not the build pin.
- Pure-Rust crypto stack — `p256` / `aes-gcm` / `hkdf` / `rustls` (ring provider, installed at startup so all rustls callers share one provider) — no OpenSSL, no `aws-lc-rs`, no `cmake`. The `rumqttc` 0.25 and `reqwest 0.13 + default rustls` paths were both attempted and rejected to preserve this invariant; the documented exception is one accepted advisory chain through `rumqttc` 0.24 + `rustls-webpki` 0.102 (CRL-only code paths not reached in default config). See `docs/SECURITY-DEBT.md` for the full ledger.
- `tower-http::request_id` mints an `x-request-id` per request, which `TraceLayer` lifts into a `request_id` span field so every log line emitted inside a handler can be grouped by it. `RAMPART_LOG_FORMAT=json` swaps the human-readable formatter for the JSON one so aggregators (Loki / Datadog / Splunk) index the field as a first-class key.

### Frontend

- Vite 8 + React 19 SPA. **One view per file** under `src/views/`. Inline CSS-in-JSX per view; no Tailwind, no CSS modules.
- All views lazy-loaded via `React.lazy()` inside a single `<Suspense>` boundary; `recharts` and `lucide-react` split into dedicated `vendor-*` chunks. Initial-page JS sits at ~200 kB (gzipped ~63 kB); charts only fetch when a chart view mounts.
- 22 e2e flows on Playwright running against Chromium / Firefox / WebKit + the branded Chrome + Edge channels = 110 cross-browser runs per CI push. Specs cover auth + 2FA, monitor CRUD + dashboard, notification channel + template CRUD, status-page public + admin, subscriber subscribe, folder + dependency edges, maintenance windows, tag routing, dark/light theme, and the per-step walkthrough screenshot generator.
- 32 vitest unit tests covering API helpers, router, theme toggle.
- Light + dark themes that auto-switch from system preference.

### Brand

- Two-tone shield + ECG-pulse mark (`docs/assets/logo.svg`) with matching wordmark lockup (`docs/assets/wordmark.svg`) for GitHub social preview / README hero.
- Favicon (inline SVG data URI — no extra HTTP request from the embedded bundle) and in-app brand mark (Dashboard header + Login card) all share the same mark.
- Step-by-step walkthrough at [`docs/WALKTHROUGH.md`](docs/WALKTHROUGH.md) covers the first-run journey with one labelled screenshot per step (11 PNGs); the screenshots are regenerated end-to-end by `npm run screenshots`.

### Security

- TLS handshake + cert-expiry probe; cert chain validated with `webpki-roots`.
- Argon2 password hashing (`argon2` crate). Failed-login costs a constant-time hash compare regardless of whether the user exists — defends against user-enumeration by timing.
- VAPID-signed Web Push (RFC 8291) using `p256` ECDSA — RFC 8291 §5 known-answer test pinned in `rampart-notifier::channels::webpush_crypto::tests::rfc8291_section5_roundtrip`.
- TOTP (RFC 6238) for 2FA, with recovery codes.
- CSRF, secure-cookie (`HttpOnly`, `SameSite=Strict`, `Secure` on HTTPS), and Argon2 defaults across the auth surface.
- CodeQL + Dependabot enabled. Dependabot groups split routine (minor + patch) / major / security per ecosystem so the routine queue stays mergeable on green.
- One advisory accepted with justification: `RUSTSEC-2026-0049/0098/0099/0104` + two GHSA aliases on `rustls-webpki` 0.102 (via `rumqttc` 0.24) — CRL paths not reached in default config; full rationale in `docs/SECURITY-DEBT.md`.

### CI / tooling

- Backend gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets --no-fail-fast`, `cargo deny check` (advisories + bans + licenses + sources).
- Frontend gates: `npm test` (vitest), `npm run build`, `npx playwright test` across 5 browser projects.
- Docker build/push split into per-arch matrix (amd64 on `ubuntu-latest`, arm64 on the native `ubuntu-24.04-arm` runner with no QEMU); each leg has a `timeout-minutes: 90` hard ceiling; per-arch buildcache refs (`:buildcache-amd64` / `:buildcache-arm64`); a merge job assembles the multi-arch manifest via `docker buildx imagetools create`.
- CodeQL workflow split per language; Rust on `build-mode: none` (the only Rust mode `codeql-action@v4` currently accepts).
- Conflict-labeler workflow creates the `has-conflicts` label idempotently so a clean clone runs without a manual bootstrap step.
- Dependency-bump policy and the living "deferred majors" table documented in [`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md).

### Out of scope (deliberately)

The following were considered and removed during the v1→v2 design pivot. Issues / PRs touching these are closed as `wontfix`:

- Multi-region distributed probing
- SLO targets / error budgets
- On-call rotations + escalation policies
- AI anomaly detection / AIOps / AI post-mortems
- Workspace multi-tenancy
- APM / RUM / log management / agent-based metrics
- Kubernetes / cloud-provider scanners

Full rationale in [`docs/DESIGN-ORIGINAL.md`](docs/DESIGN-ORIGINAL.md).

---

[Unreleased]: https://github.com/pen-pal/rampart/compare/v0.42.0...HEAD
[0.99.3]:     https://github.com/pen-pal/rampart/releases/tag/v0.99.3
[0.99.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.99.2
[0.99.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.99.1
[0.99.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.99.0
[0.98.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.98.1
[0.98.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.98.0
[0.97.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.97.0
[0.96.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.96.0
[0.95.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.95.2
[0.95.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.95.1
[0.95.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.95.0
[0.94.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.94.0
[0.93.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.93.0
[0.92.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.92.0
[0.91.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.91.0
[0.90.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.90.0
[0.89.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.89.0
[0.88.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.88.0
[0.87.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.87.0
[0.86.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.86.0
[0.85.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.85.0
[0.84.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.84.0
[0.83.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.83.0
[0.82.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.82.0
[0.81.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.81.0
[0.80.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.80.0
[0.79.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.79.0
[0.78.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.78.0
[0.77.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.77.0
[0.76.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.76.0
[0.75.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.75.0
[0.74.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.74.0
[0.73.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.73.0
[0.72.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.72.0
[0.71.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.71.0
[0.70.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.70.2
[0.70.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.70.1
[0.70.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.70.0
[0.69.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.69.0
[0.68.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.68.2
[0.68.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.68.1
[0.68.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.68.0
[0.67.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.67.2
[0.67.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.67.1
[0.67.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.67.0
[0.66.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.66.2
[0.66.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.66.1
[0.66.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.66.0
[0.65.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.65.1
[0.65.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.65.0
[0.64.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.64.1
[0.64.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.64.0
[0.63.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.63.1
[0.63.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.63.0
[0.62.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.62.1
[0.62.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.62.0
[0.61.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.61.0
[0.60.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.60.0
[0.59.4]:     https://github.com/pen-pal/rampart/releases/tag/v0.59.4
[0.59.3]:     https://github.com/pen-pal/rampart/releases/tag/v0.59.3
[0.59.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.59.2
[0.59.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.59.1
[0.59.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.59.0
[0.58.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.58.0
[0.57.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.57.1
[0.57.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.57.0
[0.56.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.56.0
[0.55.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.55.1
[0.55.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.55.0
[0.54.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.54.1
[0.54.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.54.0
[0.53.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.53.0
[0.52.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.52.0
[0.51.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.51.2
[0.51.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.51.1
[0.51.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.51.0
[0.50.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.50.0
[0.49.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.49.1
[0.49.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.49.0
[0.48.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.48.0
[0.47.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.47.0
[0.46.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.46.0
[0.45.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.45.0
[0.44.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.44.0
[0.43.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.43.0
[0.42.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.42.0
[0.41.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.41.0
[0.40.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.40.0
[0.39.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.39.0
[0.38.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.38.0
[0.37.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.37.0
[0.36.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.36.0
[0.35.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.35.0
[0.34.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.34.0
[0.33.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.33.0
[0.32.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.32.0
[0.31.2]:     https://github.com/pen-pal/rampart/releases/tag/v0.31.2
[0.31.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.31.1
[0.31.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.31.0
[0.30.1]:     https://github.com/pen-pal/rampart/releases/tag/v0.30.1
[0.30.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.30.0
[0.29.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.29.0
[0.28.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.28.0
[0.27.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.27.0
[0.26.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.26.0
[0.25.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.25.0
[0.24.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.24.0
[0.23.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.23.0
[0.22.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.22.0
[0.21.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.21.0
[0.20.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.20.0
[0.19.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.19.0
[0.18.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.18.0
[0.17.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.17.0
[0.16.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.16.0
[0.15.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.15.0
[0.14.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.14.0
[0.13.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.13.0
[0.12.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.12.0
[0.11.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.11.0
[0.10.0]:     https://github.com/pen-pal/rampart/releases/tag/v0.10.0
[0.9.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.9.0
[0.8.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.8.0
[0.7.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.7.0
[0.6.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.6.0
[0.5.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.5.0
[0.4.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.4.0
[0.3.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.3.0
[0.2.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.2.0
[0.1.0]:      https://github.com/pen-pal/rampart/releases/tag/v0.1.0

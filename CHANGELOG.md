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

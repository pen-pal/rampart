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

### Added
- Error breadcrumbs: the SDK trail leading up to an error (kept verbatim in the
  event context) now renders as a category/message/level/time timeline on the
  issue detail.

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

[Unreleased]: https://github.com/pen-pal/rampart/compare/v0.18.0...HEAD
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

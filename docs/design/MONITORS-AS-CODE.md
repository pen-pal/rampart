# Monitors-as-code (export / apply)

Define your monitors declaratively, keep them in git, and apply them through the
API — GitOps for the monitor catalog, instead of clicking through the UI. Two
endpoints, keyed by monitor **name**.

## Export

```
GET /v1/monitors/export   →   { "monitors": [ <spec>, … ] }
```

Every monitor serialized to a **spec** — the full configuration minus
server-managed fields (`id`, `created_at`/`updated_at`, `current_status`, the
runtime cert-probe results, `push_token`). What's left is exactly what `apply`
accepts, so export → commit → apply round-trips.

## Apply

```
POST /v1/monitors/apply   { "monitors": [ <spec>, … ], "prune": false }
              →   { created, updated, deleted, unchanged, errors }
```

Reconciliation, keyed by `name`:

- spec name **not** in the DB → **create**.
- spec name **exists** → **update** it in place (keeps its id + heartbeat
  history).
- DB monitor whose name is **absent** from the spec → left alone, unless
  `prune: true`, then **deleted**.

Per-item failures (a bad spec, a validation error) are collected into `errors`
and reported — one bad entry never aborts the run. The whole apply is recorded
as a single `monitors.apply` audit event with the counts.

### Names must be unique

Apply keys on `name`. Rampart doesn't enforce unique monitor names globally, so
if the DB already has two monitors sharing a name, apply can't tell which the
spec means — it reports `duplicate name in DB` for that entry and skips it.
Rename so names are unique before adopting as-code.

### Spec shape

A spec is a monitor's config object: `name`, `kind`, `url` / `hostname` / `port`,
`config`, the interval/timeout/retry fields, the `http_*` fields,
`accepted_statuses`, `follow_redirect`, `ignore_tls`, `slo_target_pct` /
`slo_window_days`, and the `proxy_id` / `group_id` / `agent_id` /
`escalation_policy_id` references. Unknown keys are ignored, so an exported spec
with extra fields still applies.

The API speaks JSON. To keep specs as YAML in git, convert in CI
(`yq -o=json`) before POSTing — Rampart deliberately doesn't take on a
(now-unmaintained) server-side YAML parser.

## Out of scope / follow-ups

- **Tags** aren't reconciled yet (stripped from export); manage them in the UI.
- Cross-instance portability of `*_id` references (proxy/group/agent/policy) is
  by id, so a spec moves cleanly only within one instance; name-based reference
  resolution is a follow-up.
- A thin `rampart apply -f monitors.json` CLI wrapper over this endpoint.

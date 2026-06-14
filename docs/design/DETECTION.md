# Detection rules (SIEM)

Detection rules turn the log tier into a SIEM signal: a saved query that
matches log records as they arrive and raises a **finding** for a SOC analyst
to triage. They are the blue-team / SIEM-team counterpart to the
[telemetry alert rules](ALERT-RULES.md) — same scheduler, different shape.

Built in migration 0090. The tables are **inert until an operator creates a
rule**: no rules, no work and no findings.

## Rule vs telemetry rule

Telemetry rules are a *sustained-breach state machine*: an aggregate crosses a
threshold and holds, fires once, then resolves. That fits "p95 latency is too
high right now".

Detection is *occurrence-based*: "N log records matched this pattern in the last
window". A finding is a record of that batch — it does not resolve, it gets
**acknowledged**. That fits "12 failed-login lines just landed".

## The match spec

A rule matches a log record when all of its set constraints hold:

| field | meaning | empty / zero = |
|---|---|---|
| `service` | exact `service_name` | any service |
| `min_level` | OTLP severity-number floor (`severity >= n`) | any severity |
| `body_regex` | case-insensitive POSIX regex on the body (Postgres `~*`) | any body |
| `attr_key` / `attr_val` | require `attributes->>attr_key = attr_val` (structured field match) | (empty key) any |

`severity` (low / medium / high / critical) is the analyst-facing label carried
onto every finding; `threshold` is how many matches in a window raise one.

`body_regex` is validated against Postgres on create/update (a bad pattern is a
`400`), so a malformed regex can never wedge the evaluation tick.

## Evaluation (the watermark)

Each slow scheduler tick calls `detection::evaluate_tick`. Per enabled rule it
counts log rows with `ts` in `(last_checked_at, now]` — or `now - window_seconds`
to `now` on the first run — that match the spec. The window bounds and the count
come from **one statement using the DB clock**, so the app/DB clocks never skew
the boundary.

If the count reaches `threshold`, the rule inserts a `detection_findings` row
(match count + the newest matching line, truncated, as a sample) and queues a
notification to its `channel_ids`. Then `last_checked_at` advances to the
window's upper bound **regardless of outcome** — so a match is counted in exactly
one finding, and the engine is restart-safe (a crash mid-tick just re-runs the
same window).

Findings dispatch through the same `send_event_to_channel` chokepoint as every
other alert (so [silences](../../README.md) apply) with `EventKind::DetectionFinding`.

## API (`/v1/detection-rules`, editor)

- `GET /v1/detection-rules` — list rules (incl. the `last_checked_at` watermark).
- `POST /v1/detection-rules` — create. Validates `body_regex` against Postgres.
- `PATCH /v1/detection-rules/{id}` / `DELETE /v1/detection-rules/{id}`.
- `GET /v1/detection-rules/findings?open=&limit=` — findings feed, newest first;
  `open=true` is the unacknowledged triage queue.
- `POST /v1/detection-rules/findings/{id}/ack` — acknowledge (idempotent).
- `POST /v1/detection-rules/preview` — dry-run a spec (`service` / `min_level` /
  `body_regex` / `window_seconds`) over recent logs without saving; returns the
  match count + up to 5 sample lines. Drives the **Preview** button on the rule
  form so an author can tune a pattern before enabling it.

## Dashboard

A `#/detection` view with two tabs: **Findings** (the triage queue, severity-
coloured, with an acknowledge action) and **Rules** (CRUD with the match spec,
severity, threshold/window, and notify channels). Editors manage; readonly
reads.

## Scope: v1 vs later

**v1 (this build):** log-tier matching (service / severity / regex), threshold +
watermark, findings feed + acknowledge, channel notifications, dashboard.

**Later:** matching on span/error tiers, attribute-key equality conditions,
grouping repeated findings, suppression windows, and exporting findings over the
[SIEM sink](SIEM.md).

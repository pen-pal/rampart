# Metrics — ingest, explore, alert

Rampart can store **metrics you push to it** — job durations, queue
depths, disk usage, anything a script can measure — and page you through
your existing notification channels when a threshold breaks. It is not a
TSDB and doesn't try to be one: Postgres-backed, homelab-scale
cardinality, age-based retention. For deep PromQL analytics keep
Prometheus; for "alert me when the queue is too deep, through the
channels I already configured" this is built in.

## Pushing samples

`POST /v1/metrics/ingest` accepts the Prometheus text exposition format
as the raw request body — anything an exporter or a shell one-liner can
produce. Authenticate with a `write`-scope API key (or an editor
session):

```bash
echo "backup_duration_seconds 312.5" | \
  curl --data-binary @- -H "Authorization: Bearer rmp_…" \
    https://rampart.example.com/v1/metrics/ingest
```

Labelled series, comments, multiple lines — all fine:

```text
# TYPE queue_depth gauge
queue_depth{queue="emails"} 42
queue_depth{queue="webhooks"} 7
disk_used_pct{mount="/var"} 81.4
```

Rules of the road:

- **Samples are server-stamped.** Timestamps in the payload are ignored —
  same trust stance as push pings and agent reports. Push at the moment
  you measure.
- `NaN` / `±Inf` samples are skipped (reported in the response's
  `skipped` count), as are unparseable lines. Max 10,000 samples per push.
- A **series** is a metric name plus its exact label set. Keep label
  cardinality sane — every distinct label combination is its own series.

## Reading back

- `GET /v1/metrics/series` — every known series with freshness and
  sample counts.
- `GET /v1/metrics/query?name=…&labels={"queue":"emails"}&from=…&to=…&step_seconds=300`
  — epoch-aligned buckets of `avg` / `min` / `max`.

The dashboard's **Metrics** view wraps both: an explorer with a chart per
series, and the rules editor below.

## Threshold alert rules

A rule watches **one series** and compares its latest sample against a
threshold:

```json
POST /v1/metrics/rules
{
  "name": "email queue too deep",
  "metric": "queue_depth",
  "labels": { "queue": "emails" },
  "op": "gt",
  "threshold": 500,
  "for_seconds": 300,
  "channel_ids": ["<notification channel uuid>", "…"]
}
```

- `op` — `gt` / `lt` / `gte` / `lte`.
- `for_seconds` — sustain window: the breach must hold continuously this
  long before the rule fires. `0` fires on the first breached evaluation.
  Evaluation runs on the scheduler's ~30s tick.
- `channel_ids` — notification channels to page, directly (monitors' tag
  routing doesn't apply — rules aren't monitors). Channel templates and
  the delivery log work as usual; rule alerts appear in the delivery log
  with event kinds `metric_rule_fired` / `metric_rule_resolved`.

Lifecycle, designed to never double-page:

1. First breached evaluation starts the sustain clock (`breach_since`).
2. Breach outlasts `for_seconds` → **fires once** (`firing_at` set) and
   notifies every channel on the rule.
3. Sample returns inside the threshold → **resolves once** with a
   recovery notice, state cleared.
4. A series that stops reporting for **15 minutes** counts as no-data and
   resolves rather than firing forever on its last stale value.

State lives on the rule row, so evaluation survives restarts. Editing a
rule resets any in-flight breach state (the old markers describe the old
condition).

## Retention

Samples are pruned by age on the hourly sweep — `metrics_days` on the
retention setting, default **30 days**. No rollup tier: telemetry past
its window is gone, like audit rows. (Heartbeat-style hourly rollups are
a possible future addition if long-horizon metric charts earn their
keep.)

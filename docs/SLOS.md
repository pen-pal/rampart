# SLOs & error budgets

Service Level Objectives with rolling **error budgets** and burn-rate alerting.
An SLO names a target — *99.9% of X succeed over 30 days* — and Rampart tracks
how much of the budget you have spent, paging before you blow it.

This is distinct from the per-monitor `slo_target_pct` field (a simple "you
promised X% uptime, you're at Y%" marker on a single monitor). An SLO is a
first-class object with its own indicator, window, budget math, and escalation.

## Model

One table, `slos` (migration 0098). Each row is an objective over one of two
**indicator kinds**:

| Kind | Achieved ratio |
| :--- | :--- |
| `monitor` | up heartbeats / total heartbeats for a monitor over the window |
| `metric`  | `SUM(good_metric) / SUM(total_metric)` over matching metric samples in the window |

For the metric kind, `good_metric` and `total_metric` are ingested counter
names (see [Metric ingestion](METRICS.md)); `labels` (JSONB) narrows the match
via containment (`@>`) — empty means *all series for that metric*.

The pure budget math lives in `rampart_core::slo`, split from the database so
it is unit-tested without I/O:

- **Error budget** — the slice of badness the objective allows.
  `99.9%` objective → `0.1%` budget.
- **Budget consumed** — `bad_ratio / error_budget`. `0%` when perfect, `100%`
  at exactly the objective, `>100%` when worse.
- **Burn rate** — how many times faster than sustainable the budget is being
  spent over a short window. `1.0` is "on pace to exactly exhaust the budget
  across the full window".

## Evaluation & paging

The scheduler evaluates every enabled SLO each tick
(`rampart_db::slos::evaluate_tick`), computing the achieved ratio over the full
window plus a 1-hour window for the fast-burn signal. An SLO is **breaching**
when either:

- the **budget is exhausted** (consumed ≥ 100% over the window), or
- it is **fast burning** — the 1-hour burn rate is ≥ **14.4×** (the Google SRE
  multi-window fast-burn threshold; at that rate a 30-day budget is gone in
  ~2 days).

Like the other rule kinds, the state machine is restart-safe and never
double-pages: a `breaching_at` marker is stamped on the first breaching tick
and cleared on recovery. On the transition into breaching it notifies the
SLO's channels and, if set, opens an episode that climbs an
[escalation policy](ESCALATIONS.md); on recovery it sends a resolve and closes
the episode.

## API

`/v1/slos` (editor scope) — standard CRUD. `GET` enriches each row with a live
snapshot so the UI renders without a second round-trip:

```json
{
  "id": "…", "name": "API request success", "sli_kind": "metric",
  "objective_pct": 99.9, "window_days": 30, "enabled": true,
  "snapshot": {
    "achieved_pct": 99.94,
    "consumed_pct": 60.0,
    "remaining_pct": 40.0,
    "burn_rate_1h": 0.6,
    "breaching": false
  }
}
```

## UI

The **SLOs** view (under *Alerting & response*) lists each objective with an
error-budget bar (green → amber → red as the budget drains), the achieved
ratio, and the 1-hour burn rate, plus an editor for the indicator, objective,
window, channels, and escalation policy.

## Try it

`rampart-api seed-demo` seeds an example metric SLO (*[demo] API request
success*, 99.9% / 30d) backed by demo `demo_req_success` / `demo_req_total`
counters, so the view has a populated budget bar to look at.

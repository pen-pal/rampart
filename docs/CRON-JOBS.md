# Cron-job monitoring

Push monitors can watch **scheduled jobs** — backups, ETL pipelines,
certificate renewals, queue sweepers — the way Cronitor or Healthchecks
do: the job pings Rampart around its run, and Rampart pages you when the
job is late, too slow, or reports failure.

## Ping states

Every push monitor has a secret ping URL (`/push/{token}`, shown on the
monitor's detail page). Three suffixes carry the run lifecycle:

| Ping | Meaning | Effect |
|------|---------|--------|
| `GET\|POST /push/{token}/run` | the job just started | opens a duration clock; records **no** heartbeat (a start is not a health sample) |
| `GET\|POST /push/{token}/complete` | finished OK | heartbeat **Up**; the run's wall-clock duration is recorded as the heartbeat's latency |
| `GET\|POST /push/{token}/fail` | finished broken | heartbeat **Down**; flips the monitor and **notifies immediately** |

`?state=run|complete|fail` works too, as does the legacy vocabulary
(`?status=up|down|warn` — `up` ≙ complete, `down` ≙ fail), plus `?msg=`
free text and `?ping=` to override the recorded duration in ms.

The classic wrapper line for a crontab:

```cron
0 3 * * * curl -fsS -m 10 $URL/run && /opt/backup.sh && curl -fsS -m 10 $URL/complete || curl -fsS -m 10 $URL/fail
```

## Schedule expectations

Without further config, a push monitor is a plain dead-man's-switch: a
ping is expected every `interval_seconds`. For jobs that run on a
*schedule*, declare it in the monitor's config (wizard fields, or the
config JSON):

```json
{
  "cron": "0 3 * * *",
  "cron_grace_seconds": 600,
  "max_run_seconds": 1800
}
```

| Key | Default | Meaning |
|-----|---------|---------|
| `cron` | — | Standard 5-field cron expression, **UTC**. Setting it switches the monitor to cron mode. |
| `cron_grace_seconds` | `300` | How late the completion ping may arrive before the run counts as **missed**. |
| `max_run_seconds` | unset | If a `/run` ping opened a run that hasn't completed within this many seconds, the monitor goes Down (**overrun**). |

Supported cron syntax: `*`, values, ranges (`1-5`), steps (`*/15`,
`1-30/5`), comma lists; day-of-week `0`–`7` with both `0` and `7` as
Sunday; the vixie rule (day-of-month OR day-of-week when both are
restricted). Month/day names are not supported — use numbers.

### Cron-mode semantics

- The scheduler only synthesizes heartbeats when an expectation breaks:
  - **missed run** — the most recent scheduled slot passed by more than
    the grace with no terminal ping since the slot:
    `missed scheduled run at 2026-06-11T03:00:00Z UTC (grace 600s)`.
  - **overrun** — an open run exceeded `max_run_seconds`:
    `run exceeded max duration: 2100s (limit 1800s)`.
- The healthy timeline belongs to the job's own pings — uptime numbers
  reflect real runs, and a `fail` ping's Down is never overwritten by a
  scheduler tick (an interval-mode quirk cron mode fixes).
- Scheduled slots from before the monitor existed don't count — a
  monitor created at 10:05 isn't "late" for the 10:00 run.
- Maintenance windows suppress everything, same as any other monitor.

### Duration tracking

When a run is opened with `/run` and closed with `/complete`, the
heartbeat's latency value is the job's wall-clock duration — so the
monitor's response-time chart doubles as a **run-duration chart**, and
slow drift is visible at a glance.

## Choosing interval vs cron mode

| | interval mode (no `cron`) | cron mode |
|---|---|---|
| Expectation | a ping every `interval_seconds` | a completion per scheduled slot (+ grace) |
| Good for | heartbeat daemons, "I'm alive" loops | crontabs, schedulers, CI nightlies |
| Timeline | synthesized Up/Down every tick | the job's own pings + failure synthesis |
| Failure ping | flips Down, then overwritten Up by next tick (legacy quirk) | flips Down and stays until the next successful run |

Note: in cron mode the monitor's `interval_seconds` no longer expresses
the expectation — it only sets how often the scheduler re-evaluates it
(60s is plenty).

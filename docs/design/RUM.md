# Real User Monitoring (Tier 4)

Status: **implemented (v1)**. See [`docs/ROADMAP.md`](../ROADMAP.md) Tier 4 —
the last tier, completing the observability platform.

RUM measures the experience of **real browsers**: Core Web Vitals (LCP, INP,
CLS) plus FCP, TTFB, and load time, per page. Competes with Datadog RUM /
Sentry web-vitals, self-hosted.

## The snippet

Rampart serves a tiny self-installing collector at **`GET /rum/snippet.js`**.
One tag installs it:

```html
<script src="https://<rampart-host>/rum/snippet.js" data-app="web"></script>
```

It reads `data-app` (names the site; default `web`) and optional
`data-endpoint`, collects vitals via `PerformanceObserver` (LCP, CLS, INP) +
Navigation Timing (TTFB, FCP, load), and sends **one beacon on page hide** via
`navigator.sendBeacon` — no dependency, no build step, ~1 KB. Because
`sendBeacon` uses a simple `text/plain` request, there's no CORS preflight.

## Ingest

`POST /rum/v1/events` — one beacon: `{ app, url, session?, ua?, metrics }`,
where `metrics` is any subset of `{ lcp, fcp, cls, inp, ttfb, load }`. Public
(beacons come from arbitrary browsers); the body is parsed as JSON regardless
of content-type. A beacon with no URL or no metric is silently dropped (204) —
browsers ignore the response, so ingest never errors. Stored one row per view
(`rum_events`, migration 0080) with a `rum_days` retention window (default 14)
in the prune sweep.

## Read API (`/v1/rum`, editor/readonly)

- `GET /v1/rum/summary?app=&hours=` — **p75** of each metric over the window
  (p75 is the standard Web Vitals statistic), plus the view count.
- `GET /v1/rum/pages?app=&hours=` — per-URL rollup (views + p75 LCP/INP/CLS),
  busiest first.
- `GET /v1/rum/apps` — distinct app/site names for the filter.

p75 is computed in Postgres with `percentile_cont(0.75) WITHIN GROUP`.

## Dashboard

A `#/rum` view: Core Web Vitals cards (p75 LCP/INP/CLS + FCP/TTFB), each
coloured **good / needs-improvement / poor** against the official thresholds
(`rampart_core::rum::cwv_good_threshold` is the shared source of truth), a
per-page table with the same vitals, an app + time-window filter, and the
copyable install snippet.

## Follow-ups (deferred)

- Per-app keys/CRUD (v1 keys by an `app` name in the beacon, no table).
- JS-error capture feeding the error-tracking tier; session/user dimensions;
  geo/device breakdowns; histograms beyond p75.
- INP is approximated (max event duration) — the full INP algorithm is a
  follow-up.

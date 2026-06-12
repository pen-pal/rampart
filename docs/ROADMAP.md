# Rampart roadmap

> Where Rampart is going, why, and the principles that decide what we say no to.
> This doc is both the engineering sequencing plan and the pitch for why
> Rampart is worth self-hosting, contributing to, and sponsoring.

## The bet

Commercial observability (Datadog, Sentry, Site24x7, ScoutAPM, Honeybadger,
New Relic) is powerful and expensive, and it puts your operational data on
someone else's servers under per-host / per-event / per-seat pricing that
punishes you for growing. Rampart's bet:

> **One open-source binary that covers uptime, synthetics, status pages,
> on-call, errors, traces, and logs — self-hosted, AGPL, you own the data.**

We will not win a feature-parity race against Datadog at Datadog's scale, and
we are not trying to. We win for the very large middle of the market —
homelabs, indie devs, startups, and small-to-mid teams — who want serious
reliability tooling without a SaaS bill or a data-exfiltration clause, and who
would rather `docker compose up` than operate a Kubernetes-scale telemetry
cluster.

Reference playbook: Uptime Kuma, Plausible, PostHog, GlitchTip — open-source
products that took a category owned by expensive SaaS and won the self-hosted
segment on simplicity, price (free), and data ownership.

## Principles (these decide the trade-offs)

1. **AGPL-3.0, and it stays that way.** Anyone can self-host, fork, and
   contribute. Nobody can take Rampart, run it as a closed competing SaaS, and
   keep their changes private. This is the moat: it points commercial energy at
   *sponsoring and contributing* rather than freeloading. Funding is community
   sponsorship (GitHub Sponsors / Open Collective), not a license sale.
2. **One binary, easy self-host, forever.** Frontend embedded via `rust-embed`,
   ~10 MB stripped binary, Postgres the only hard dependency. Every feature
   must preserve "`docker compose up` and you have it." A feature that forces a
   homelab user to operate ClickHouse/Cassandra/Kafka to get basic value is a
   failed feature. Heavy stores may be *optional* scale-out, never the floor.
3. **Single-tenant by design.** No multi-tenant SaaS control plane, no
   `workspace_id` scoping. The unit of deployment is one team's instance. This
   keeps the code small and the security model simple. (Projects/namespaces
   *within* an instance — e.g. for error tracking — are fine; that's not
   tenancy.)
4. **Postgres-default storage with retention tiering.** Telemetry is bounded by
   time-based retention windows and pruned, not kept forever. We reach for a
   specialised store only when Postgres genuinely can't carry a tier at
   small-team volume, and only as an opt-in.
5. **No C/crypto-toolchain dependencies.** Pure-Rust deps only (see
   [`docs/DEPENDENCIES.md`](DEPENDENCIES.md)). Keeps the build and the
   single-binary story clean.
6. **Reuse the spine.** Every new tier rides the existing notification,
   escalation/on-call, delivery-log, and status-page machinery instead of
   reinventing alerting per tier.

## Where Rampart is today (shipped)

The **uptime + alerting** core is mature:

- 38 probe kinds (HTTP family, TCP/DNS/ping/TLS, databases, message brokers,
  service-discovery, banner protocols, domain expiry, browser-rendered).
- Push / cron-job monitoring (Cronitor-style run states + schedule awareness).
- Remote probe agents (multi-location / private-network) + host metrics.
- External metric ingest (Prometheus text) + threshold alert rules.
- Multi-step **synthetic transactions** (HTTP sequences, variable extraction,
  assertions).
- Escalation policies + **on-call schedules/rotations**.
- Status pages, maintenance windows, incident templates, SLO tracking.
- Notification channels (many kinds), templates, quiet hours, digests,
  delivery log, result webhooks.

This already covers most of **Site24x7** (uptime/synthetics/network) and the
uptime + cron half of **Honeybadger**.

## The tiers ahead

Ordered by leverage: user value per unit of effort, weighted toward what draws
contributors and sponsors. Each tier is a months-scale effort, not a
weekend feature.

### Tier 1 — Error & exception tracking  ← next
**Competes with:** Sentry (core), Honeybadger, GlitchTip, Bugsnag.

Capture exceptions from running apps, group them into issues by fingerprint,
dedupe the flood, alert on new/regressed issues, and show stack traces. This is
the **highest-leverage next tier**: enormous unmet demand for a simple
self-hosted Sentry, bounded scope, fits Postgres with retention pruning, and it
reuses the existing alert/notify/escalation spine.

Key design decision: **be Sentry-DSN/envelope compatible on ingest**, so
existing official Sentry SDKs (JS, Python, Rust, Go, …) point at Rampart with
only a DSN change — zero SDK to build, instant ecosystem. Full design:
[`docs/design/ERROR-TRACKING.md`](design/ERROR-TRACKING.md).

### Tier 2 — APM / distributed tracing (OTLP)
**Competes with:** ScoutAPM, Datadog APM, Sentry Performance, New Relic.

Ingest OpenTelemetry spans (OTLP), reconstruct traces, render a service map and
latency/throughput/error breakdowns, surface slow spans and N+1-style patterns.
Bigger lift: needs a span store, tail/head sampling, and trace assembly.
Standardising on OTLP means we ingest from the whole OpenTelemetry SDK
ecosystem rather than shipping our own agents.

### Tier 3 — Log ingestion & search
**Competes with:** Datadog Logs, Grafana Loki, Honeycomb.

Structured-log ingest, retention, and query/filter, correlated to issues and
traces by ids. Highest-volume tier — this is where Postgres-only is most
likely to need an opt-in columnar/object-store backend behind a retention
tier. Sequenced after traces because trace context is what makes logs
navigable.

### Tier 4 — Real User Monitoring (RUM)
**Competes with:** Datadog RUM, Sentry (web vitals / session replay).

A browser SDK reporting Core Web Vitals, page loads, JS errors (feeds Tier 1),
and route timings. Frontend-heavy; depends on the error and trace tiers
existing first.

## What we deliberately are NOT building

- Multi-tenant SaaS control plane / billing / per-seat metering.
- Datadog-scale ingest clusters as a *requirement* (opt-in scale-out only).
- AI/ML anomaly detection as a headline feature (a pragmatic baseline-drift
  alert is fine; a "smart" black box is not).
- Closed-source "enterprise edition." Sponsorship funds the work; the code
  stays AGPL and whole.

## How this funds itself

The product *is* the marketing: a one-command install, honest docs, and
"Rampart vs <expensive SaaS>, self-hosted" comparison pages turn cost-conscious
teams into users, users into GitHub stars and contributors, and a fraction of
those into sponsors via GitHub Sponsors / Open Collective. AGPL ensures that
companies depending on Rampart have a reason to fund it rather than privatise
it. Sustainability = many small sponsors + a healthy contributor base, not a
sales team.

---

*This roadmap is intentionally a living document. Tier ordering can shift with
contributor interest and sponsor demand, but the principles above are the
stable part — they're how we decide.*

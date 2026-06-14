# Continuous profiling & flamegraphs

> **Status: proposed (v0.11.0).** This document is the design for the profiling
> tier. The one decision that gates implementation — the **ingest format** — is
> called out in [Ingest](#ingest); everything downstream of it (storage, read
> API, render) is settled.

The fifth telemetry tier, alongside errors / traces / logs / RUM. Where traces
answer *"which request was slow and in what span,"* profiling answers *"which
**code** burned the CPU / allocated the memory,"* aggregated across many samples
into a **flamegraph**. This is the ScoutAPM / Datadog Profiler / Pyroscope /
Parca capability — the on-CPU (and alloc) view that turns "the API is slow" into
"`serde_json::from_slice` is 38% of CPU in `rampart-api`."

It stays true to Rampart's identity: single binary, self-hosted, Postgres-backed,
opt-in. No separate columnar store, no agent fleet required — point any profiler
that speaks the wire format at the ingest endpoint, the same way OTLP traces and
logs already work.

## What a profile is

A profile is a set of **samples**. Each sample is a **stack** (a list of frames,
leaf-last or root-last) plus one or more **values** (e.g. `cpu_nanos`,
`alloc_bytes`, `sample_count`). A CPU profiler samples the call stack ~100×/sec;
each sample contributes its stack with `value = 1` (or the sampling period in
nanos). Merging thousands of samples and grouping by shared stack prefixes gives
the flamegraph: width = share of total value, depth = call depth.

The universal lowest-common-denominator representation is a **folded stack map**:

```
rampart_api::main;tokio::runtime;serde_json::from_slice   3812
rampart_api::main;tokio::runtime;sqlx::query::execute     1190
```

— one line per unique stack, `;`-joined frames, trailing integer value. Every
flamegraph renderer (inferno, flamegraph.pl, d3-flame-graph, speedscope) consumes
this, and every profiler can be made to emit it. It is our **internal storage
and render format**; the ingest wire format converts *into* it.

## Ingest

**This is the decision to confirm before building.** Three candidate wire
formats, in order of recommendation:

### Proposed: pprof (primary) + folded text (secondary)

**pprof** — Google's `profile.proto` (gzipped protobuf) — is the de-facto
standard. It is emitted, natively or via a one-line converter, by **Go
`runtime/pprof`, the Rust `pprof` crate, py-spy, async-profiler, .NET, the
Pyroscope/Grafana SDKs, and Parca/Polar Signals agents.** Accepting pprof means a
user can point an existing profiler at Rampart on day one — no bespoke
instrumentation. The format is self-contained (sample types, locations,
functions, line numbers, a string table), so we get symbolized frames for free
when the producer symbolized them.

- `POST /profiles/v1/pprof?service=<name>&type=<cpu|alloc_space|...>` — body is a
  gzipped pprof. Honors the same optional **telemetry token** + rate limit as
  `/otlp` and `/rum` (see [LOGS](LOGS.md) / ingest auth). Parsed server-side with
  `prost` + the vendored `profile.proto` into our folded map.

A **folded-stack text** endpoint rides alongside for the trivial / scripted case
(profilers without a pprof path, or a `perf script | stackcollapse-perf.pl`
pipeline):

- `POST /profiles/v1/folded?service=&type=` — body is `stack value\n` lines,
  stored verbatim. Twenty lines of parser, zero dependencies.

### Deferred: OTLP profiles

The OpenTelemetry **profiling signal** is pprof-shaped and would align with our
existing OTLP traces/logs ingest, but native SDK/agent emission is still thin
(2025). Because our storage is folded stacks (a pprof superset-of-need), adopting
OTLP-profiles later is an *additive* ingest route, not a migration. Recommended
forward path once the ecosystem catches up.

> **Why not a custom JSON format?** Nothing emits it. The whole value of the tier
> is "works with the profiler you already run." pprof is that lingua franca.

## Storage

One table, `profiles` (migration `008X`), sibling to `traces` / `logs`:

| column | type | note |
|---|---|---|
| `id` | bigserial | |
| `received_at` | timestamptz | ingest time; drives retention + the time filter |
| `service_name` | text | scope (the OTLP `service.name` analogue) |
| `profile_type` | text | `cpu` / `alloc_space` / `inuse_space` / `wall` / … |
| `period_ns` | bigint | sampling period (for value→time scaling) |
| `duration_ns` | bigint | wall span the profile covers |
| `sample_count` | int | merged sample total (cheap list-view stat) |
| `labels` | jsonb | producer tags (host, version, pid, k8s pod, …) |
| `folded` | bytea | gzipped folded-stack map — the render source |

Retention rides the existing **hourly prune loop** (a `profiles_retention_days`
setting beside `heartbeats` / `audit_log`). Profiles are the heaviest tier
per-row, so the default is short (7 days) and the prune is by `received_at`.

Storing the merged folded map (not raw pprof) keeps rows compact and render
instant; we drop the producer's raw bytes after parsing. Frame **symbolization**
is the producer's job (pprof carries function names) — matching how we store JS
stacks raw in the error tier rather than symbolicating server-side (a documented
follow-up there too).

## Read API & flamegraph

- `GET /v1/profiles?service=&type=&hours=` — list profiles in the window
  (timestamp, type, sample count, labels) for the picker.
- `GET /v1/profiles/flamegraph?service=&type=&from=&to=` — **merge** every folded
  map in the window into a single tree and return it as nested
  `{name, value, children[]}`. Merging server-side means the browser ships a tree,
  not megabytes of samples.
- `GET /v1/profiles/{id}/flamegraph` — the same tree for one profile.

The dashboard adds a **Profiling** view (`#/profiling`): service + type + window
pickers, a flamegraph (icicle layout — root on top — rendered as inline SVG with
hover tooltips and click-to-zoom into a subtree), and a sibling **top functions**
table (self vs total value, the tabular companion to the visual, mirroring the
APM Operations tab). No new JS dependency — the merge tree is small and the SVG
rects are a `<div>`/`<svg>` map, consistent with how the trace waterfall and
service map are drawn today.

A later pass can wire **trace → profile** correlation (jump from a slow span to
the profile covering that window/service), closing the loop with the APM tier the
same way logs↔traces already pivot.

## Follow-ups (explicitly out of v1)

- OTLP-profiles ingest route (see above).
- Trace↔profile correlation (span → covering profile).
- Diff flamegraphs (A/B two windows — "what got slower since the deploy").
- Server-side symbolization for unsymbolized native profiles (needs debug-info
  upload; large scope, like source-map symbolication in the error tier).
- Profile-type alerting (e.g. "CPU in `service X` function `Y` > N% sustained").

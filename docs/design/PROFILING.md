# Continuous profiling & flamegraphs

> **Status: shipped (v0.11.0).** All three ingest formats below are live —
> folded text, pprof, and OTLP profiles — over the folded-stack storage model,
> with the merge read API and the icicle flamegraph + top-functions view.

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

All three wire formats are accepted; each is lowered to a folded map and stored
identically. Every ingest route honors the optional **telemetry token** + the
ingest IP rate limit (see [LOGS](LOGS.md) / ingest auth) and `Content-Encoding`.

### pprof — `POST /profiles/v1/pprof`

**pprof** — Google's `profile.proto` (gzipped protobuf) — is the de-facto
standard, emitted natively or via a one-line converter by **Go `runtime/pprof`,
the Rust `pprof` crate, py-spy, async-profiler, .NET, the Pyroscope/Grafana SDKs,
and Parca/Polar Signals agents**. Point an existing profiler at Rampart on day
one — no bespoke instrumentation. Decoded server-side with a hand-written `prost`
subset of `profile.proto` (no heavy profiler dependency); the profile type
defaults to the pprof sample-type name, period/duration come from the profile.
Query: `service`, `type`.

### OTLP profiles — `POST /otlp/v1development/profiles`

The OpenTelemetry **profiling signal**, on the same `/otlp` surface as traces +
logs. Accepts an `ExportProfilesServiceRequest`; one request may carry many
profiles (resources/scopes), each lowered independently; `service.name` comes
from the resource. The signal is still `v1development` and the `opentelemetry-
proto` crate's generated types lag the wire format, so we hand-write the current
`v1development` subset as `prost` types (the "dictionary" model: shared
location/function/string tables + per-sample index slices).

### Folded text — `POST /profiles/v1/folded`

The trivial / scripted path (a `perf script | stackcollapse-perf.pl` pipeline, or
any profiler without a pprof exporter): body is `stack value\n` lines. ~20 lines
of parser, zero dependencies. Query: `service`, `type`, `period_ns`,
`duration_ns`.

> **Why not a custom JSON format?** Nothing emits it. The whole value of the tier
> is "works with the profiler you already run." pprof + OTLP are the lingua
> franca; folded text is the universal escape hatch.

## Storage

One table, `profiles` (migration `0084`), sibling to `traces` / `logs`:

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
- `GET /v1/profiles/flamegraph/diff?service=&type=&hours=` — **diff** the most
  recent window against the immediately preceding one; each node carries its
  after value and the after−before `delta` (positive = hotter), so "what got
  slower since the deploy" reads straight off the colors.

The dashboard adds a **Profiling** view (`#/profiling`): service + type + window
pickers, a flamegraph (icicle layout — root on top — `<div>` cells with hover
tooltips and click-to-zoom into a subtree), a **Diff** toggle (red = hotter /
blue = colder vs the preceding window), and a sibling **top functions** table
(self vs total value, the tabular companion to the visual, mirroring the APM
Operations tab). No new JS dependency — the merge tree is small and the cells are
a positioned-`<div>` map, consistent with the trace waterfall + service map.

**Trace → profile** correlation is wired: a trace's detail links its root service
to that service's flamegraph (`#/profiling?service=<svc>`), closing the loop with
the APM tier the same way logs↔traces already pivot.

## Follow-ups

- Server-side symbolization for unsymbolized native profiles (needs debug-info
  upload; large scope, like source-map symbolication in the error tier).
- Profile-type alerting (e.g. "CPU in `service X` function `Y` > N% sustained").
- Absolute-time trace↔profile correlation (a past span → the exact profile
  covering its window, not just the service).

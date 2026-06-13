# Log ingestion (Tier 3)

Status: **implemented (v1)**. See [`docs/ROADMAP.md`](../ROADMAP.md) Tier 3.
Builds on the OTLP ingest foundation from the traces tier.

Rampart ingests OpenTelemetry **logs** over OTLP, stores them with their
optional `trace_id`/`span_id`, and serves a filtered log stream — the logs
layer of the observability platform (Datadog Logs / Loki / GlitchTip class),
self-hosted.

## Ingest

`POST /otlp/v1/logs` accepts an OTLP `ExportLogsServiceRequest` in **both**
encodings (chosen by `Content-Type`): OTLP/JSON (`application/json`, parsed by a
tolerant hand-rolled parser in `rampart_core::log` reusing the trace tier's
shared OTLP attribute helpers) and OTLP/protobuf (`application/x-protobuf`, via
`opentelemetry-proto`, lowered to the same `ParsedLog`). Mounted at the root
`/otlp` surface, outside the session layer (single-tenant self-host; operator
controls exposure). Point an OTel SDK/Collector logs exporter at
`http://<host>/otlp`.

`Content-Encoding: gzip`/`deflate` bodies are transparently inflated, and the
optional shared **ingest token** (Settings → Ingest token) gates this endpoint
the same way as the traces tier (`Authorization: Bearer`/`X-Rampart-Token`).
Unsampled — all records are stored, bounded by retention.

## Storage & model

One row per record (`logs` table, migration 0079): event time, OTLP severity
number (1–24) + text, service, body, optional `trace_id`/`span_id` (the
correlation key back to a trace), and flattened attributes (JSONB). A coarse
level (trace/debug/info/warn/error/fatal) is derived from the severity number
on read. Logs age out via a `logs_days` retention window (default 7) folded
into the prune sweep — the highest-volume tier, so retention is short.

## Read API (`/v1/logs`, editor/readonly)

- `GET /v1/logs?service=&level=&q=&trace_id=&limit=` — recent logs, newest
  first. `level` is a *minimum* coarse level (e.g. `warn` → warn+error+fatal,
  translated to a severity-number threshold); `q` is a case-insensitive
  substring match on the body; `trace_id` pulls a single trace's logs.
- `GET /v1/logs/services` — distinct recent service names for the filter UI.

## Dashboard

A `#/logs` view: a filter bar (service dropdown, minimum-level dropdown, body
search) over a compact log stream — timestamp, level (colour-coded), service,
body — with a click-to-expand row revealing `trace_id`/`span_id`, the exporter
severity text, and attributes.

## Follow-ups (deferred)

- Full-text search (Postgres `tsvector`) instead of `ILIKE` substring.
- Live tail (SSE), and cross-tier nav: trace detail → its logs (by `trace_id`),
  error issue → correlated logs.
- A plain-JSON bulk ingest for non-OTel sources; volume controls (sampling /
  drop rules) and an opt-in columnar/object store if Postgres is outgrown.

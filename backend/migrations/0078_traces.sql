-- Distributed tracing — OTLP span ingest (APM tier, Tier 2). See
-- docs/design/TRACES.md (planned). Stores individual OpenTelemetry spans;
-- a "trace" is the set of spans sharing a trace_id, assembled on read. A
-- span with no parent_span_id (or whose parent isn't present) is a root.
--
-- Single-tenant, self-host: ingest is mounted unauthenticated at
-- /otlp/v1/traces (the operator controls network exposure, like a Prometheus
-- scrape target). Spans age out via a retention window, like metric samples.
CREATE TABLE spans (
    -- OTLP ids are hex (trace_id = 32 hex chars, span_id = 16). Kept as text
    -- so the OTLP/JSON form stores verbatim and joins stay simple.
    span_id        TEXT        PRIMARY KEY,
    trace_id       TEXT        NOT NULL,
    parent_span_id TEXT        NULL,
    service_name   TEXT        NOT NULL DEFAULT 'unknown',
    name           TEXT        NOT NULL,
    -- OTLP SpanKind: 0 unspecified, 1 internal, 2 server, 3 client, 4 producer, 5 consumer.
    kind           SMALLINT    NOT NULL DEFAULT 0,
    start_ns       BIGINT      NOT NULL,
    end_ns         BIGINT      NOT NULL,
    duration_ms    DOUBLE PRECISION NOT NULL,
    -- OTLP StatusCode: 0 unset, 1 ok, 2 error.
    status_code    SMALLINT    NOT NULL DEFAULT 0,
    status_message TEXT        NULL,
    -- span + resource attributes, flattened to a JSON object.
    attributes     JSONB       NULL,
    received_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Assemble a trace (all spans with one trace_id).
CREATE INDEX spans_trace_idx ON spans (trace_id);
-- Recent-traces listing + the retention prune.
CREATE INDEX spans_recent_idx ON spans (received_at DESC);
-- Service-map + per-service rollups.
CREATE INDEX spans_service_idx ON spans (service_name, received_at DESC);

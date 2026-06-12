-- Log ingestion (Tier 3). See docs/design/LOGS.md.
--
-- Stores individual log records ingested via OTLP (/otlp/v1/logs). Each row
-- carries OTLP severity, the message body, the originating service, and —
-- crucially — the optional trace_id / span_id so logs correlate with the
-- traces tier (jump from a span to its logs). High-volume, short retention.
CREATE TABLE logs (
    id            UUID        PRIMARY KEY,
    -- Event time (OTLP timeUnixNano); falls back to observed time.
    ts            TIMESTAMPTZ NOT NULL,
    -- OTLP severity number (1–24); 0 = unspecified. severity_text is the
    -- exporter's label ("INFO"/"ERROR"); coarse level derived on read.
    severity      SMALLINT    NOT NULL DEFAULT 0,
    severity_text TEXT        NULL,
    service_name  TEXT        NOT NULL DEFAULT 'unknown',
    body          TEXT        NOT NULL DEFAULT '',
    -- Correlation with the traces tier (hex ids); NULL for non-traced logs.
    trace_id      TEXT        NULL,
    span_id       TEXT        NULL,
    attributes    JSONB       NULL,
    received_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Recent-logs stream + the retention prune.
CREATE INDEX logs_recent_idx ON logs (received_at DESC);
-- Per-service filtering.
CREATE INDEX logs_service_idx ON logs (service_name, received_at DESC);
-- Jump from a trace/span to its logs.
CREATE INDEX logs_trace_idx ON logs (trace_id) WHERE trace_id IS NOT NULL;

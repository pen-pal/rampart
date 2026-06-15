-- Correlate RUM page-loads with the backend trace they triggered.
--
-- The browser beacon can carry the active trace id (e.g. from the document's
-- traceparent or a traced first fetch). Nullable — most page-loads won't have
-- one, and that's fine; when present it lets the RUM view deep-link to the
-- trace waterfall, closing the RUM → trace side of the cross-tier links.
ALTER TABLE rum_events ADD COLUMN trace_id TEXT NULL;

CREATE INDEX rum_trace_idx ON rum_events (trace_id) WHERE trace_id IS NOT NULL;

-- Extract the trace id from error events into an indexed column so traces can
-- find their errors (the reverse of the existing error→trace link). Populated
-- at ingest from the Sentry event's contexts.trace.trace_id; NULL when the SDK
-- didn't attach a trace. Backfilled from the stored context for existing rows.
ALTER TABLE error_events ADD COLUMN trace_id TEXT;

UPDATE error_events
SET trace_id = context #>> '{contexts,trace,trace_id}'
WHERE trace_id IS NULL AND context #>> '{contexts,trace,trace_id}' IS NOT NULL;

CREATE INDEX error_events_trace_id_idx ON error_events (trace_id) WHERE trace_id IS NOT NULL;

-- Full-text search over log bodies.
--
-- The logs view's body filter was a `body ILIKE '%q%'` substring scan — fine
-- at low volume, but it can't use an index and degrades as the table grows.
-- Add a generated `tsvector` column (maintained by Postgres, no trigger) plus
-- a GIN index, so the query can switch to `body_tsv @@ websearch_to_tsquery`.
-- 'english' is a reasonable default config; log bodies are usually English-ish
-- and the stemming/stop-word handling is harmless for identifier-heavy text.
ALTER TABLE logs
    ADD COLUMN body_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('english', body)) STORED;

CREATE INDEX logs_body_tsv_idx ON logs USING GIN (body_tsv);

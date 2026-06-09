-- Incident de-duplication key for webhook-driven ingest.
--
-- Inbound webhook receivers (Alertmanager / Grafana / Datadog) need an
-- exact handle to match the "resolved/recovered" payload back to the
-- "firing/triggered" incident they opened. Matching by title is fragile:
-- two distinct alerts can share a title, so a resolve could close the
-- wrong incident.
--
-- `dedup_key` stores a vendor-supplied stable identifier (Alertmanager /
-- Grafana `fingerprint`, Datadog `alert_id`) at firing time. On resolve we
-- look up the active incident on the same page with the matching key.
--
-- The partial unique index keeps at most one *active* incident per
-- (page, key) so a duplicate firing doesn't stack incidents, while
-- allowing many resolved rows to retain the same historical key.
ALTER TABLE incidents
    ADD COLUMN dedup_key TEXT;

CREATE UNIQUE INDEX incidents_dedup_key_active_idx
    ON incidents (status_page_id, dedup_key)
    WHERE dedup_key IS NOT NULL AND active;

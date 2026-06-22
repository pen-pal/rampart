-- Detection engine + log_volume now window on logs.received_at (server clock)
-- instead of the client-supplied ts (defeats timeUnixNano backdating). Those
-- queries filter `org_id = $ AND received_at <time>`; add the (org_id,
-- received_at DESC) composite so org-scoped detection seeks its tenant slice
-- (mirrors spans/rum/profiles composites from 0111) instead of falling back to
-- the single-column logs_received_at_idx then filtering org_id.
CREATE INDEX IF NOT EXISTS logs_org_received_at_idx ON logs (org_id, received_at DESC);

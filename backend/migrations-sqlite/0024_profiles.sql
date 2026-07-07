-- Multi-DB P2 (SQLite) — continuous-profiling tier. Ported from PG/MySQL. One
-- row per ingested profile; `folded` holds the gzipped folded-stack map (the
-- uniform internal form every wire format is lowered into). Flamegraphs are
-- assembled on read by merging the folded maps in a time window. Dialect:
-- BIGSERIAL→INTEGER PRIMARY KEY AUTOINCREMENT, ts→INTEGER unix-seconds,
-- JSONB→TEXT, BYTEA→BLOB.

CREATE TABLE profiles (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  received_at  INTEGER NOT NULL DEFAULT (unixepoch()),
  service_name TEXT    NOT NULL,
  profile_type TEXT    NOT NULL,
  period_ns    INTEGER NOT NULL DEFAULT 0,
  duration_ns  INTEGER NOT NULL DEFAULT 0,
  sample_count INTEGER NOT NULL DEFAULT 0,
  labels       TEXT    NOT NULL,
  folded       BLOB    NOT NULL,
  org_id       TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX profiles_service_type_time_idx ON profiles (service_name, profile_type, received_at);
CREATE INDEX profiles_received_at_idx ON profiles (received_at);

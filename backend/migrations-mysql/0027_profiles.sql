-- Multi-DB P2 (MySQL) — continuous-profiling tier. Ported from PG (0084). One
-- row per ingested profile; `folded` holds the gzipped folded-stack map (the
-- uniform internal form every wire format is lowered into). Flamegraphs are
-- assembled on read by merging the folded maps in a time window. uuid→CHAR(36),
-- ts→BIGINT, BIGSERIAL→BIGINT AUTO_INCREMENT, JSONB→LONGTEXT, BYTEA→LONGBLOB.

CREATE TABLE profiles (
  id           BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  received_at  BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  service_name VARCHAR(255) NOT NULL,
  profile_type VARCHAR(64)  NOT NULL,
  period_ns    BIGINT       NOT NULL DEFAULT 0,
  duration_ns  BIGINT       NOT NULL DEFAULT 0,
  sample_count INT          NOT NULL DEFAULT 0,
  labels       LONGTEXT     NOT NULL,
  folded       LONGBLOB     NOT NULL,
  org_id       CHAR(36)     NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX profiles_service_type_time_idx ON profiles (service_name, profile_type, received_at);
CREATE INDEX profiles_received_at_idx ON profiles (received_at);

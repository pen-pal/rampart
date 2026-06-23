-- Multi-DB P2 (MySQL) — external metric samples (read foundation for
-- metric_rules + slos). Forked from PG/SQLite. jsonb/canonical-TEXT labels →
-- TEXT with **utf8mb4_bin** collation so `=` and `GROUP BY` are byte-exact
-- (utf8mb4's default collation is case-/accent-insensitive, which would merge
-- distinct label sets — series identity must be exact). double→DOUBLE,
-- ts→BIGINT. PG/SQLite have no PK; MySQL/InnoDB gets a surrogate AUTO_INCREMENT
-- `id` (also the same-second tie-break for `latest`, replacing SQLite rowid).

CREATE TABLE metric_samples (
  id     BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  name   VARCHAR(255) NOT NULL,
  labels TEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  value  DOUBLE       NOT NULL,
  ts     BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX metric_samples_name_ts_idx ON metric_samples (name, ts DESC);
CREATE INDEX metric_samples_org_idx ON metric_samples (org_id);

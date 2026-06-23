-- Multi-DB P2 (MySQL) — periodic uptime-digest reports. Forked from PG/SQLite.
-- TEXT[] recipients → LONGTEXT(JSON), ts→BIGINT, uuid→CHAR(36).

CREATE TABLE scheduled_reports (
  id           CHAR(36)     NOT NULL PRIMARY KEY,
  name         VARCHAR(255) NOT NULL,
  recipients   LONGTEXT     NOT NULL,
  cadence      VARCHAR(16)  NOT NULL DEFAULT 'weekly',
  last_sent_at BIGINT,
  created_at   BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id       CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX scheduled_reports_org_idx ON scheduled_reports (org_id);

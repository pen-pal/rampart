-- Multi-DB P2 (MySQL) — outbound proxy configs for probe routing. Forked from
-- PG/SQLite. uuid→CHAR(36), bool→TINYINT, ts→BIGINT, port i32→INT.

CREATE TABLE proxies (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  protocol   VARCHAR(16)  NOT NULL
                          CHECK (protocol IN ('http', 'https', 'socks', 'socks5', 'socks4')),
  host       VARCHAR(255) NOT NULL,
  port       INT          NOT NULL CHECK (port BETWEEN 1 AND 65535),
  auth       TINYINT      NOT NULL DEFAULT 0,
  username   VARCHAR(255),
  password   TEXT,
  active     TINYINT      NOT NULL DEFAULT 1,
  org_id     CHAR(36)     NOT NULL REFERENCES organizations(id),
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX proxies_org_idx ON proxies (org_id);

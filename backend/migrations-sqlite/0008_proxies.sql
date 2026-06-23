-- Multi-DB P1 (SQLite) — outbound proxies (probe routing).
--
-- Forked from PG 0001 (proxies) + 0108/0112 (org_id NOT NULL end-state).
-- Dialect: uuid→TEXT, bool→INTEGER 0/1, timestamptz→INTEGER unix-seconds. The
-- protocol + port CHECKs port verbatim. `password` is stored as-is (the proxy
-- domain does no sealing — the route layer owns any encryption, same as PG).

CREATE TABLE proxies (
  id         TEXT    PRIMARY KEY,
  protocol   TEXT    NOT NULL CHECK (protocol IN ('http','https','socks','socks5','socks4')),
  host       TEXT    NOT NULL,
  port       INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
  auth       INTEGER NOT NULL DEFAULT 0,
  username   TEXT,
  password   TEXT,
  active     INTEGER NOT NULL DEFAULT 1,
  org_id     TEXT    NOT NULL REFERENCES organizations(id),
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX proxies_org_idx ON proxies (org_id);

-- Multi-DB P2 (SQLite) — Real User Monitoring (Tier 4). Ported from PG/MySQL.
-- One row per page-view beacon: the URL, optional app/session/ua, an optional
-- backend trace id, and the Core Web Vitals + load timings (any subset; NULL
-- when not measured). Dialect: uuid→TEXT, ts→INTEGER unix-seconds, DOUBLE→REAL.
-- High-volume; short retention (rum_days). Read-side p75 aggregation is app-side
-- (see sqlite/rum.rs), same as the MySQL tier.

CREATE TABLE rum_events (
  id          TEXT    NOT NULL PRIMARY KEY,
  ts          INTEGER NOT NULL DEFAULT (unixepoch()),
  app         TEXT    NOT NULL DEFAULT 'web',
  url         TEXT    NOT NULL,
  session     TEXT,
  ua          TEXT,
  trace_id    TEXT,
  user_id     TEXT,
  lcp_ms      REAL,
  fcp_ms      REAL,
  cls         REAL,
  inp_ms      REAL,
  ttfb_ms     REAL,
  load_ms     REAL,
  received_at INTEGER NOT NULL DEFAULT (unixepoch()),
  org_id      TEXT    NOT NULL REFERENCES organizations(id)
);
CREATE INDEX rum_recent_idx ON rum_events (received_at);
CREATE INDEX rum_app_idx ON rum_events (app, received_at);
CREATE INDEX rum_url_idx ON rum_events (url);

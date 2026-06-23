-- Multi-DB P2 (MySQL) — Real User Monitoring (Tier 4). Ported from PG (0080 +
-- the org_id/trace_id/user_id follow-ups). One row per page-view beacon: the
-- URL, optional app/session/ua, an optional backend trace id, and the Core Web
-- Vitals + load timings (any subset; NULL when not measured). uuid→CHAR(36),
-- ts→BIGINT, DOUBLE PRECISION→DOUBLE. High-volume; short retention.
--
-- Read-side p75 aggregation is done app-side (MySQL has no `percentile_cont`
-- aggregate; MariaDB only exposes it as a window function) — see mysql/rum.rs.

CREATE TABLE rum_events (
  id          CHAR(36)      NOT NULL PRIMARY KEY,
  ts          BIGINT        NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  app         VARCHAR(255)  NOT NULL DEFAULT 'web',
  url         VARCHAR(2048) NOT NULL,
  session     VARCHAR(255)  NULL,
  ua          TEXT          NULL,
  trace_id    VARCHAR(64)   NULL,
  user_id     VARCHAR(255)  NULL,
  lcp_ms      DOUBLE        NULL,
  fcp_ms      DOUBLE        NULL,
  cls         DOUBLE        NULL,
  inp_ms      DOUBLE        NULL,
  ttfb_ms     DOUBLE        NULL,
  load_ms     DOUBLE        NULL,
  received_at BIGINT        NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id      CHAR(36)      NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX rum_recent_idx ON rum_events (received_at);
CREATE INDEX rum_app_idx    ON rum_events (app, received_at);
CREATE INDEX rum_url_idx     ON rum_events (url(255));

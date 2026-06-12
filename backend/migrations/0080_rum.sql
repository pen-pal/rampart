-- Real User Monitoring (Tier 4). See docs/design/RUM.md.
--
-- One row per page-view beacon from the browser RUM snippet: the URL, an
-- optional app/site name + anonymous session, and the Core Web Vitals + load
-- timings collected for that view (any subset; NULL when not measured). The
-- read side aggregates p75 per metric (the standard Web Vitals reporting
-- statistic). High-volume; short retention.
CREATE TABLE rum_events (
    id          UUID        PRIMARY KEY,
    ts          TIMESTAMPTZ NOT NULL DEFAULT now(),
    app         TEXT        NOT NULL DEFAULT 'web',
    url         TEXT        NOT NULL,
    session     TEXT        NULL,
    ua          TEXT        NULL,
    -- Core Web Vitals + navigation timings, milliseconds (cls is unitless).
    lcp_ms      DOUBLE PRECISION NULL,
    fcp_ms      DOUBLE PRECISION NULL,
    cls         DOUBLE PRECISION NULL,
    inp_ms      DOUBLE PRECISION NULL,
    ttfb_ms     DOUBLE PRECISION NULL,
    load_ms     DOUBLE PRECISION NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX rum_recent_idx  ON rum_events (received_at DESC);
CREATE INDEX rum_app_idx     ON rum_events (app, received_at DESC);
CREATE INDEX rum_url_idx      ON rum_events (app, url);

-- Deploy markers: point-in-time annotations (deploys, config changes) overlaid
-- on the metric / response-time timelines for change-correlation (MTTR).
CREATE TABLE deploy_markers (
    id          uuid PRIMARY KEY,
    ts          timestamptz NOT NULL DEFAULT now(),
    title       text NOT NULL,
    description text,
    service     text,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- Listing is always "markers in a time window, newest first".
CREATE INDEX deploy_markers_ts_idx ON deploy_markers (ts DESC);

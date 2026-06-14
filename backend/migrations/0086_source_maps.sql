-- JS source maps for server-side symbolication of error-tier stack frames.
-- Keyed by (project, release, filename): a minified frame's basename + the
-- event's release selects the map; the `sourcemap` crate resolves (line,col) to
-- the original function/file/line on read. See docs/design/ERROR-TRACKING.md.
CREATE TABLE source_maps (
    id          BIGSERIAL   PRIMARY KEY,
    project_id  UUID        NOT NULL REFERENCES error_projects(id) ON DELETE CASCADE,
    release     TEXT        NOT NULL,
    -- basename of the minified file the frame references (e.g. main.abc123.js)
    filename    TEXT        NOT NULL,
    map         JSONB       NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, release, filename)
);

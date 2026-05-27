-- Maintenance windows.
--
-- A window is a [start_at, end_at) time range during which any monitor
-- attached to it is reported as Maintenance instead of Up/Down. The
-- scheduler checks this on every tick and suppresses both the probe
-- itself and any notification fan-out.
--
-- v1 is single-shot only (no recurrence). The schema leaves room to add
-- a JSON `recurrence` column later without a breaking change.

CREATE TABLE IF NOT EXISTS maintenance_windows (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    start_at    TIMESTAMPTZ NOT NULL,
    end_at      TIMESTAMPTZ NOT NULL,
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT  maintenance_windows_range CHECK (end_at > start_at)
);

CREATE INDEX IF NOT EXISTS maintenance_windows_active_range_idx
    ON maintenance_windows (active, start_at, end_at);

-- M2M: a window covers one or more monitors. When either side goes away
-- the join row goes with it.
CREATE TABLE IF NOT EXISTS maintenance_window_monitors (
    window_id  UUID NOT NULL REFERENCES maintenance_windows(id) ON DELETE CASCADE,
    monitor_id UUID NOT NULL REFERENCES monitors(id)            ON DELETE CASCADE,
    PRIMARY KEY (window_id, monitor_id)
);

CREATE INDEX IF NOT EXISTS mwm_monitor_idx ON maintenance_window_monitors (monitor_id);

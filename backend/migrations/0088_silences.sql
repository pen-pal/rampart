-- Alert silences: suppress notifications during a window (deploys, known noise).
-- monitor_id NULL = global (mute everything, incl. rule alerts). A non-NULL
-- monitor_id mutes just that monitor's alerts. expires_at NULL = until removed.
-- Checked at the notifier dispatch chokepoint. See docs/design/ALERT-RULES.md.
CREATE TABLE silences (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    monitor_id UUID        REFERENCES monitors(id) ON DELETE CASCADE,
    reason     TEXT        NOT NULL DEFAULT '',
    created_by UUID        REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ
);

-- The hot path is "is anything silencing this monitor right now".
CREATE INDEX silences_active_idx ON silences (expires_at, monitor_id);

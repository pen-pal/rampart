-- Escalation policies + episodes (on-call tier, batch-20).
--
-- A policy is an ordered ladder of notification steps:
--   steps = [ { "wait_seconds": 0,   "channel_ids": ["…"] },
--             { "wait_seconds": 600, "channel_ids": ["…"] }, … ]
-- Step 0 fires the moment a monitor with this policy flips Down; each
-- later step fires `wait_seconds` after the previous one, unless someone
-- ACKNOWLEDGES the episode or the monitor recovers first. Steps live as
-- validated JSONB — policies are small, ordered, and always read whole.
--
-- A monitor that references a policy routes its Down-flips through the
-- ladder INSTEAD of the regular attached/tag-routed channel fan-out;
-- recovery notifies every step already paged. Monitors without a policy
-- are completely untouched.
CREATE TABLE escalation_policies (
    id         UUID        PRIMARY KEY,
    name       TEXT        NOT NULL,
    steps      JSONB       NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE monitors
    ADD COLUMN escalation_policy_id UUID NULL
        REFERENCES escalation_policies(id) ON DELETE SET NULL;

-- One episode = one open "this monitor is down and the ladder is
-- climbing" incident. The partial unique index makes "at most one open
-- episode per monitor" a database invariant, so a double Down-flip
-- (e.g. flapping) can't open two ladders.
CREATE TABLE escalation_episodes (
    id                 UUID        PRIMARY KEY,
    monitor_id         UUID        NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
    policy_id          UUID        NOT NULL REFERENCES escalation_policies(id) ON DELETE CASCADE,
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Highest step index already fired (step 0 fires at open).
    last_step          INT         NOT NULL DEFAULT 0,
    -- When the next step is due; NULL once the ladder is exhausted.
    next_escalation_at TIMESTAMPTZ NULL,
    acked_at           TIMESTAMPTZ NULL,
    acked_by           UUID        NULL REFERENCES users(id) ON DELETE SET NULL,
    resolved_at        TIMESTAMPTZ NULL
);

CREATE UNIQUE INDEX escalation_episodes_open_idx
    ON escalation_episodes (monitor_id) WHERE resolved_at IS NULL;

-- The scheduler's advance scan: open, unacked, due.
CREATE INDEX escalation_episodes_due_idx
    ON escalation_episodes (next_escalation_at)
    WHERE resolved_at IS NULL AND acked_at IS NULL AND next_escalation_at IS NOT NULL;

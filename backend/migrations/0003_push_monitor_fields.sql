-- Push monitor support.
--
-- Push monitors are passive: the external job calls POST /push/:token to
-- signal "I'm alive". The probe runner is special-cased — instead of
-- making outbound calls, the scheduler compares NOW() to last_push_at
-- and marks the monitor down if no push arrived inside the interval
-- (with a small grace window).
--
-- push_token is a random URL-safe string assigned at monitor-create time
-- for monitors of kind 'push'. Other kinds carry NULL.

ALTER TABLE monitors
  ADD COLUMN IF NOT EXISTS push_token   TEXT,
  ADD COLUMN IF NOT EXISTS last_push_at TIMESTAMPTZ;

-- Unique when set so the lookup in the /push/:token handler is O(1).
CREATE UNIQUE INDEX IF NOT EXISTS monitors_push_token_idx
  ON monitors (push_token)
  WHERE push_token IS NOT NULL;

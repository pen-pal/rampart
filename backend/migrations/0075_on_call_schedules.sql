-- On-call schedules — channel rotations (on-call tier, batch-21).
--
-- A schedule is an ordered ring of notification channels that hand off on
-- a fixed cadence:
--   participant_ids  = ["chan-a", "chan-b", "chan-c"]   (rotation order)
--   rotation_seconds = 604800                            (weekly handoff)
--   anchor           = 2026-06-15T09:00:00Z              (chan-a's shift starts)
-- "Who is on call at instant T" is pure arithmetic — no per-shift rows to
-- pre-generate or keep pruned:
--   period = floor((T - anchor) / rotation_seconds)
--   on_call = participant_ids[ period mod len(participant_ids) ]
-- so the ring is resolvable for any T, past or future, including before the
-- anchor (it wraps backward, which is well-defined).
--
-- Rampart's notification model addresses by CHANNEL, not by user (there is
-- no user↔contact link), so a rotation rotates channels. An escalation step
-- references a schedule by id (in its JSONB `schedule_ids`); at page time the
-- ladder resolves the schedule to its current on-call channel and pages it
-- alongside the step's fixed `channel_ids`. A schedule that no longer
-- resolves (deleted, emptied) is simply skipped — same as an unresolvable
-- channel id. Participants live as a validated JSONB array (small, ordered,
-- always read whole), mirroring escalation `steps`/`channel_ids`.
CREATE TABLE on_call_schedules (
    id               UUID        PRIMARY KEY,
    name             TEXT        NOT NULL,
    -- Rotation length in seconds; each participant holds the pager for this
    -- long before handing off to the next in the ring.
    rotation_seconds BIGINT      NOT NULL,
    -- The instant participant[0]'s first shift begins. All handoffs are
    -- computed as offsets from this anchor (UTC).
    anchor           TIMESTAMPTZ NOT NULL,
    -- Ordered notification channel ids in rotation order (JSONB array).
    participant_ids  JSONB       NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

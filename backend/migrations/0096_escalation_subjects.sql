-- Generalize escalation episodes from monitors to arbitrary "subjects" so alert
-- rules (not just monitors) can climb a timed ladder. A subject is
-- (subject_kind, subject_ref): for a monitor it's ('monitor', monitor_id);
-- for a telemetry rule ('telemetry_rule', rule_id). monitor_id stays (now
-- nullable) for the FK cascade on monitor delete; rule episodes leave it NULL.
ALTER TABLE escalation_episodes
    ALTER COLUMN monitor_id DROP NOT NULL,
    ADD COLUMN subject_kind TEXT NOT NULL DEFAULT 'monitor',
    ADD COLUMN subject_ref  TEXT;

-- Backfill existing (all monitor) episodes.
UPDATE escalation_episodes SET subject_ref = monitor_id::text WHERE subject_ref IS NULL;
ALTER TABLE escalation_episodes ALTER COLUMN subject_ref SET NOT NULL;

-- Replace the "one open episode per monitor" invariant (0074's
-- escalation_episodes_open_idx) with one per subject.
DROP INDEX IF EXISTS escalation_episodes_open_idx;
CREATE UNIQUE INDEX escalation_episodes_open_subject_uniq
    ON escalation_episodes (subject_kind, subject_ref)
    WHERE resolved_at IS NULL;

-- Telemetry alert rules can route through an escalation policy: on fire, the
-- policy's recipients (every step's channels + on-call schedules, incl. the
-- current on-call person) are paged in addition to the rule's own channels.
-- (This fans out to the whole ladder on fire; the timed per-step climb for
-- rule subjects is a follow-up — see docs.)
ALTER TABLE telemetry_alert_rules
    ADD COLUMN escalation_policy_id UUID NULL
        REFERENCES escalation_policies(id) ON DELETE SET NULL;

-- Metric + detection rules can route through an escalation policy, same as
-- telemetry rules (0095). On fire (metric) or finding (detection) an episode
-- opens and climbs; metric rules resolve on their RuleTransition::Resolve,
-- detection episodes resolve when no finding lands within a grace window.
ALTER TABLE metric_rules
    ADD COLUMN escalation_policy_id UUID NULL
        REFERENCES escalation_policies(id) ON DELETE SET NULL;

ALTER TABLE detection_rules
    ADD COLUMN escalation_policy_id UUID NULL
        REFERENCES escalation_policies(id) ON DELETE SET NULL;

-- Per-rule suppression window for detection rules. After a finding is raised,
-- the rule won't raise another until cooldown_seconds have elapsed — stops a
-- sustained match stream from firing a finding every scheduler tick. 0 = no
-- cooldown (legacy behavior); existing rules keep it so nothing changes for them.
ALTER TABLE detection_rules
    ADD COLUMN cooldown_seconds int NOT NULL DEFAULT 0;

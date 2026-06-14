-- Optional attribute-key match for detection rules: match a log record only
-- when its JSONB `attributes` has `attr_key` equal to `attr_val`. Empty
-- `attr_key` = no attribute constraint (the existing behaviour). Lets a rule
-- key off a structured field (e.g. event.action = "user.delete") rather than a
-- body regex. See docs/design/DETECTION.md.
ALTER TABLE detection_rules
    ADD COLUMN attr_key TEXT NOT NULL DEFAULT '',
    ADD COLUMN attr_val TEXT NOT NULL DEFAULT '';

-- Detection v2: boolean condition tree. When non-null, supersedes the flat
-- service/min_level/body_regex/attr_* match fields — the eval layer compiles it
-- to a parameterized SQL WHERE. Null = legacy flat match (existing rules
-- unchanged).
ALTER TABLE detection_rules
    ADD COLUMN condition jsonb;

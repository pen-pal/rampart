-- Detection v2: per-entity aggregation. `group_by` names a log attribute; when
-- set, the tick groups matches by attributes->>group_by and raises one finding
-- per entity reaching the threshold. The finding records which entity fired.
ALTER TABLE detection_rules
    ADD COLUMN group_by text NOT NULL DEFAULT '';
ALTER TABLE detection_findings
    ADD COLUMN entity text;

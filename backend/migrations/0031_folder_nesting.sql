-- Nested folders: a folder may live inside another folder.
--
-- parent_id NULL = a root folder (renders at the dashboard's top level).
-- ON DELETE SET NULL so deleting a parent promotes its children to root
-- rather than cascading them away — same demote-don't-destroy stance as
-- a monitor losing its group. Cycle prevention (a folder can't be its own
-- ancestor) is enforced in application code, mirroring monitor_dependencies.

ALTER TABLE monitor_groups
    ADD COLUMN IF NOT EXISTS parent_id UUID
        REFERENCES monitor_groups(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS monitor_groups_parent_idx
    ON monitor_groups (parent_id);

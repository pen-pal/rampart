-- Monitor groups + dependencies.
--
-- Groups: cosmetic. A monitor belongs to at most one group. NULL = "no
-- group" and renders in the dashboard's default bucket. Deleting a group
-- demotes its monitors to NULL rather than deleting them — destructive
-- cascades are surprising and groups are easy to recreate.
--
-- Dependencies: an alert-routing concern, not a probing one. We still
-- probe a child monitor when its parent is Down — losing visibility
-- because Postgres is down doesn't actually mean the API is fine — but
-- we suppress the *notification* for the child so a single root-cause
-- failure doesn't paging-storm every dependent service. The dashboard
-- shows the child with a "blocked-by" badge so operators see why no
-- page fired.

CREATE TABLE IF NOT EXISTS monitor_groups (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS monitor_groups_sort_idx
    ON monitor_groups (sort_order, name);

ALTER TABLE monitors
    ADD COLUMN IF NOT EXISTS group_id UUID
        REFERENCES monitor_groups(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS monitors_group_idx ON monitors (group_id);

-- Self-referential M2M. PK enforces no duplicates; the CHECK prevents a
-- monitor from depending on itself (Postgres won't dig deeper than a
-- one-hop cycle here — application code rejects multi-hop cycles).
CREATE TABLE IF NOT EXISTS monitor_dependencies (
    monitor_id    UUID NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
    depends_on_id UUID NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (monitor_id, depends_on_id),
    CONSTRAINT monitor_dependencies_no_self_loop
        CHECK (monitor_id <> depends_on_id)
);

CREATE INDEX IF NOT EXISTS monitor_dependencies_parent_idx
    ON monitor_dependencies (depends_on_id);

-- Fix: the status-page DB layer (rampart-db::status_pages) queries a flat
-- `status_page_monitors (page_id, monitor_id, position)` table, but the
-- initial schema only shipped the vestigial groups/components model. The
-- table existed on dev DBs (from a migration that was later dropped) so
-- the sqlx offline cache + local runs worked, but every fresh deploy hit
-- `relation "status_page_monitors" does not exist` → 500 on status-page
-- create. Recreate it here so fresh installs match the code.
--
-- IF NOT EXISTS keeps this a no-op on DBs that already have it.

CREATE TABLE IF NOT EXISTS status_page_monitors (
    page_id    UUID NOT NULL REFERENCES status_pages(id) ON DELETE CASCADE,
    monitor_id UUID NOT NULL REFERENCES monitors(id)     ON DELETE CASCADE,
    position   INT  NOT NULL DEFAULT 0,
    PRIMARY KEY (page_id, monitor_id)
);
CREATE INDEX IF NOT EXISTS spm_monitor_idx   ON status_page_monitors (monitor_id);
CREATE INDEX IF NOT EXISTS spm_page_pos_idx  ON status_page_monitors (page_id, position);

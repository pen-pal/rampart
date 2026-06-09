-- Status-page component grouping (sub-sections).
--
-- Operators want to group the flat list of attached monitors into labelled
-- sections ("API", "Database", "Edge") on the public page. We model this as
-- a thin `status_page_sections` table owned by a page, plus a nullable
-- `section_id` on the existing `status_page_monitors` join.
--
--   status_page_sections   one row per labelled group on a page. `position`
--                          orders the sections on the public page; CASCADE
--                          on the page FK so deleting a page drops its
--                          sections.
--
--   status_page_monitors.section_id
--                          which section a monitor renders under. NULL means
--                          the monitor is ungrouped — it renders at the top
--                          of the components list, above the first section
--                          header. Existing rows default to NULL, so every
--                          current page keeps rendering its flat list
--                          untouched until an operator assigns sections.
--
-- The FK uses ON DELETE SET NULL: deleting a section un-assigns its monitors
-- (they fall back to ungrouped) rather than detaching them from the page.

CREATE TABLE IF NOT EXISTS status_page_sections (
    id             UUID PRIMARY KEY,
    status_page_id UUID NOT NULL REFERENCES status_pages(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    position       INT  NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS sps_page_pos_idx
    ON status_page_sections (status_page_id, position);

ALTER TABLE status_page_monitors
    ADD COLUMN IF NOT EXISTS section_id UUID
        REFERENCES status_page_sections(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS spm_section_idx
    ON status_page_monitors (section_id);

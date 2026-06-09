-- Incident-update templates — canned incident text an operator can apply
-- instead of retyping the same phases over and over.
--
-- Operators repeat the same handful of status-page incident updates on
-- every incident: "Investigating", "Identified", "Monitoring",
-- "Resolved". Rather than retype the body + pick the visual style each
-- time in the StatusPageBuilder, they save a small library of templates
-- and apply one with a click.
--
-- Intentionally GLOBAL, not per-page: the phrasing of "we are
-- investigating" is the same on every status page an org runs, and a
-- shared library is one fewer thing to manage. No status_page_id column.
--
-- `style` reuses the existing `incident_style` enum so an applied
-- template carries its colour straight into the incident form.

CREATE TABLE incident_templates (
    id         UUID PRIMARY KEY,
    name       TEXT           NOT NULL,
    body       TEXT           NOT NULL,
    style      incident_style NOT NULL DEFAULT 'warning',
    created_at TIMESTAMPTZ    NOT NULL DEFAULT now()
);

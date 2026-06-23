-- Multi-DB P2 (MySQL) — per-org notification-template name uniqueness, matching
-- PG migration 0113 (a name is unique within an org, not globally). Lets the
-- templates CRUD surface a friendly Conflict on a duplicate name.

CREATE UNIQUE INDEX notification_templates_org_name_uidx
  ON notification_templates (org_id, name);

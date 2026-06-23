-- Multi-DB P2 (MySQL) management-API tail — incident-update templates (canned
-- incident bodies). Ported from PG. uuid→CHAR(36), ts→BIGINT, `incident_style`
-- enum→VARCHAR (serde round-trip, like monitors.kind). org_id ref documentary.

CREATE TABLE incident_templates (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  name       VARCHAR(255) NOT NULL,
  body       TEXT         NOT NULL,
  style      VARCHAR(16)  NOT NULL DEFAULT 'warning',
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id     CHAR(36)     NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX incident_templates_org_idx ON incident_templates (org_id);

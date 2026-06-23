-- Multi-DB P2 (MySQL) — escalation policies + the episode state machine.
-- Forked from PG/SQLite. uuid→CHAR(36), jsonb steps→LONGTEXT, ts→BIGINT.
--
-- The "one OPEN episode per subject" invariant is a PARTIAL unique index in
-- PG/SQLite (`WHERE resolved_at IS NULL`), which MySQL can't express. The MySQL
-- idiom: a STORED generated `open_key` = subject while open, NULL once resolved
-- (NULLs don't collide), with a plain UNIQUE on it — so an INSERT for an
-- already-open subject fails the unique check (atomic, same semantics), and
-- resolving frees the slot (the generated column recomputes to NULL).

CREATE TABLE escalation_policies (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  name       VARCHAR(255) NOT NULL,
  steps      LONGTEXT     NOT NULL,
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id     CHAR(36)     NOT NULL REFERENCES organizations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX escalation_policies_org_idx ON escalation_policies (org_id);

CREATE TABLE escalation_episodes (
  id                 CHAR(36)    NOT NULL PRIMARY KEY,
  monitor_id         CHAR(36)    REFERENCES monitors(id) ON DELETE CASCADE,
  policy_id          CHAR(36)    NOT NULL REFERENCES escalation_policies(id) ON DELETE CASCADE,
  started_at         BIGINT      NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  last_step          INT         NOT NULL DEFAULT 0,
  next_escalation_at BIGINT,
  acked_at           BIGINT,
  acked_by           CHAR(36)    REFERENCES users(id) ON DELETE SET NULL,
  resolved_at        BIGINT,
  subject_kind       VARCHAR(32) NOT NULL DEFAULT 'monitor',
  subject_ref        VARCHAR(64) NOT NULL,
  open_key           VARCHAR(120) GENERATED ALWAYS AS (
                       CASE WHEN resolved_at IS NULL
                            THEN CONCAT(subject_kind, ':', subject_ref) END
                     ) STORED,
  UNIQUE KEY escalation_episodes_open_subject_uniq (open_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

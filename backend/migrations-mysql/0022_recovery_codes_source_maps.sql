-- Multi-DB P2 (MySQL) management-API tail — two small self-contained domains in
-- one migration (batched to cut rebuild/ship churn). Ported from PG.
-- `release` is a MySQL reserved word → backticked in DDL + all queries. The
-- project_id / user_id refs stay documentary (no enforced FK): error_projects
-- isn't ported yet, and recovery codes clean up via delete_for_user.

CREATE TABLE totp_recovery_codes (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  user_id    CHAR(36)     NOT NULL,
  code_hash  VARCHAR(128) NOT NULL,
  used_at    BIGINT,
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX trc_user_unused_idx ON totp_recovery_codes (user_id);

CREATE TABLE source_maps (
  id          BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  project_id  CHAR(36)     NOT NULL,
  `release`   VARCHAR(255) NOT NULL,
  filename    VARCHAR(512) NOT NULL,
  map         LONGTEXT     NOT NULL,
  uploaded_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  UNIQUE (project_id, `release`, filename)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

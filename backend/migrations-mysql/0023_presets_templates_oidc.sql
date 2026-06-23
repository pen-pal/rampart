-- Multi-DB P2 (MySQL) management-API tail — 3 small standalone domains batched.
-- Ported from PG. uuid→CHAR(36), JSONB→LONGTEXT, ts→BIGINT. `state` TEXT PK →
-- VARCHAR(64) (a 48-char token; MySQL can't PK an unbounded TEXT). org_id refs
-- documentary (no enforced FK, per convention). oidc_login_state is pre-auth
-- cross-tenant (not org-scoped).

CREATE TABLE monitor_presets (
  id         CHAR(36)     NOT NULL PRIMARY KEY,
  name       VARCHAR(255) NOT NULL,
  kind       VARCHAR(32)  NOT NULL CHECK (kind IN ('http_headers', 'tls')),
  data       LONGTEXT     NOT NULL,
  created_at BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id     CHAR(36)     NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX monitor_presets_org_idx ON monitor_presets (org_id);

CREATE TABLE monitor_templates (
  id          CHAR(36)     NOT NULL PRIMARY KEY,
  name        VARCHAR(255) NOT NULL,
  description TEXT,
  spec        LONGTEXT     NOT NULL,
  created_at  BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  org_id      CHAR(36)     NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX monitor_templates_org_idx ON monitor_templates (org_id);

CREATE TABLE oidc_login_state (
  state         VARCHAR(64) NOT NULL PRIMARY KEY,
  pkce_verifier TEXT        NOT NULL,
  nonce         TEXT,
  return_to     TEXT,
  created_at    BIGINT      NOT NULL DEFAULT (UNIX_TIMESTAMP()),
  expires_at    BIGINT      NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX idx_oidc_login_state_expires_at ON oidc_login_state (expires_at);

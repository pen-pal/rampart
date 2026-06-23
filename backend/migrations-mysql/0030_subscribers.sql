-- Multi-DB P2 (MySQL) — status-page email subscribers. Ported from PG. v1 is
-- single-opt-in (the row lands confirmed). Page-scoped (org tenanting flows
-- through the owning status page). uuid→CHAR(36), ts→BIGINT, bool→TINYINT.

CREATE TABLE status_page_subscribers (
  id             CHAR(36)     NOT NULL PRIMARY KEY,
  status_page_id CHAR(36)     NOT NULL,
  channel        VARCHAR(16)  NOT NULL DEFAULT 'email',
  destination    VARCHAR(320) NOT NULL,
  confirmed      TINYINT(1)   NOT NULL DEFAULT 0,
  confirm_token  VARCHAR(64)  NULL,
  subscribed_at  BIGINT       NOT NULL DEFAULT (UNIX_TIMESTAMP())
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX status_page_subscribers_page_idx  ON status_page_subscribers (status_page_id);
CREATE INDEX status_page_subscribers_token_idx ON status_page_subscribers (confirm_token);
CREATE INDEX status_page_subscribers_dest_idx  ON status_page_subscribers (destination);

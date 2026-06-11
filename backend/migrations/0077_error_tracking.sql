-- Error & exception tracking (Tier 1 — Sentry-lite). See
-- docs/design/ERROR-TRACKING.md.
--
-- A `error_project` is a namespace for one app's errors (not tenancy — a
-- folder within the single instance, like monitor groups). Its public_key is
-- the DSN key SDKs embed; it is NOT secret (Sentry's model — the key ships in
-- browser JS), so it's stored plaintext and ingest is gated by per-project
-- rate limiting, not key secrecy.
--
-- Events are grouped into `error_issues` by fingerprint (same bug → one
-- issue), so a crash loop is one row with a counter, not N rows. The
-- (project_id, fingerprint) unique index makes the grouping a DB invariant and
-- lets ingest upsert-and-bump in a single statement. `error_events` holds the
-- per-occurrence detail and ages out via retention pruning; issues persist.
CREATE TABLE error_projects (
    id             UUID        PRIMARY KEY,
    name           TEXT        NOT NULL,
    slug           TEXT        NOT NULL UNIQUE,
    public_key     TEXT        NOT NULL UNIQUE,
    platform       TEXT        NULL,
    retention_days INT         NOT NULL DEFAULT 30,
    -- Notification channel ids paged on a new / regressed issue (JSONB array,
    -- mirroring escalation steps / metric rules).
    alert_channel_ids JSONB    NOT NULL DEFAULT '[]',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE error_issues (
    id          UUID        PRIMARY KEY,
    project_id  UUID        NOT NULL REFERENCES error_projects(id) ON DELETE CASCADE,
    fingerprint TEXT        NOT NULL,
    title       TEXT        NOT NULL,
    culprit     TEXT        NULL,
    level       TEXT        NOT NULL DEFAULT 'error',
    -- unresolved | resolved | ignored
    status      TEXT        NOT NULL DEFAULT 'unresolved',
    first_seen  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen   TIMESTAMPTZ NOT NULL DEFAULT now(),
    times_seen  BIGINT      NOT NULL DEFAULT 0,
    assignee    UUID        NULL REFERENCES users(id) ON DELETE SET NULL
);

-- One issue per (project, fingerprint) — the grouping invariant + the
-- ON CONFLICT target for the ingest upsert.
CREATE UNIQUE INDEX error_issues_group_idx ON error_issues (project_id, fingerprint);
-- Issue list is "newest activity first, by project".
CREATE INDEX error_issues_recent_idx ON error_issues (project_id, last_seen DESC);

CREATE TABLE error_events (
    id             UUID        PRIMARY KEY,
    issue_id       UUID        NOT NULL REFERENCES error_issues(id) ON DELETE CASCADE,
    project_id     UUID        NOT NULL REFERENCES error_projects(id) ON DELETE CASCADE,
    ts             TIMESTAMPTZ NOT NULL,
    level          TEXT        NOT NULL DEFAULT 'error',
    message        TEXT        NULL,
    exception_type TEXT        NULL,
    culprit        TEXT        NULL,
    environment    TEXT        NULL,
    release        TEXT        NULL,
    server_name    TEXT        NULL,
    -- normalized stack frames
    stacktrace     JSONB       NULL,
    -- tags + user + request + extra + the raw event payload, kept verbatim
    context        JSONB       NULL
);

-- Event detail per issue, newest first; and a project-wide time index for the
-- retention prune.
CREATE INDEX error_events_issue_idx ON error_events (project_id, issue_id, ts DESC);
CREATE INDEX error_events_prune_idx ON error_events (project_id, ts);

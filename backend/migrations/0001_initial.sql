-- Rampart · initial schema (single-tenant, multi-user)
--
-- One install, one team, no workspaces. Postgres-only by design — the
-- embedded-DB option was considered and dropped after weighing reliability
-- under load.
--
-- Notable design choices reflected in the schema:
--   notification_templates  shared body templates across channels
--   maintenance             windows with single/recurring/cron strategies
--   monitor_kind 'domain'   daily WHOIS-based expiry check
--   status_pages.auto_degrade  auto-flip page status on incident
--   status_badges           per-monitor SVG badges with logos
--   audit_log               who-did-what record
--
-- Postgres 14+. Run via `sqlx migrate run`.

CREATE EXTENSION IF NOT EXISTS "citext";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ─── users + sessions ─────────────────────────────────────────────────────

CREATE TABLE users (
  id              UUID         PRIMARY KEY,
  email           CITEXT       UNIQUE NOT NULL,
  name            TEXT,
  password_hash   TEXT         NOT NULL,
  totp_secret     TEXT,                              -- base32, NULL if 2FA off
  is_admin        BOOLEAN      NOT NULL DEFAULT FALSE,
  created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  last_login_at   TIMESTAMPTZ
);

CREATE TABLE sessions (
  id              UUID         PRIMARY KEY,
  user_id         UUID         NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  expires_at      TIMESTAMPTZ  NOT NULL,
  ip_addr         INET,
  user_agent      TEXT
);
CREATE INDEX sessions_user_idx    ON sessions (user_id);
CREATE INDEX sessions_expires_idx ON sessions (expires_at);

-- ─── proxies (outbound HTTP/SOCKS) ────────────────────────────────────────
-- For monitors that must egress through a proxy. Declared before monitors
-- because monitors.proxy_id references it.

CREATE TABLE proxies (
  id            UUID         PRIMARY KEY,
  protocol      TEXT         NOT NULL CHECK (protocol IN ('http','https','socks','socks5','socks4')),
  host          TEXT         NOT NULL,
  port          INT          NOT NULL CHECK (port BETWEEN 1 AND 65535),
  auth          BOOLEAN      NOT NULL DEFAULT FALSE,
  username      TEXT,
  password      TEXT,                                 -- encrypted app-side
  active        BOOLEAN      NOT NULL DEFAULT TRUE,
  created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- ─── monitors ─────────────────────────────────────────────────────────────

CREATE TYPE monitor_kind AS ENUM (
  -- HTTP family
  'http',          -- plain HTTP/S status check
  'keyword',       -- HTTP body must contain string
  'json_query',    -- HTTP body matches a JSONPath assertion
  -- network primitives
  'tcp',           -- TCP port open
  'ping',          -- ICMP ping
  'dns',           -- DNS resolve, optional expected value
  'push',          -- inbound heartbeat; the user's job pings us
  'grpc',          -- gRPC health check (grpc.health.v1)
  'tls',           -- cert-only check, no body fetch
  -- service-specific
  'docker',        -- Docker container running (via socket)
  'steam',         -- Steam game server
  'mqtt',          -- MQTT subscribe
  'radius',        -- RADIUS auth check
  'kafka',         -- Kafka producer
  -- databases
  'postgres', 'mysql', 'mssql', 'redis', 'mongodb',
  -- registry
  'domain'         -- WHOIS-based domain registration expiry
);

CREATE TYPE monitor_status AS ENUM
  ('up','down','warn','paused','pending','maintenance');

CREATE TABLE monitors (
  id                   UUID            PRIMARY KEY,
  name                 TEXT            NOT NULL,
  kind                 monitor_kind    NOT NULL,
  -- Endpoint addressing — used by kind. http kinds use `url`; tcp/ping/dns
  -- use `hostname` (+ optional `port`). `config` holds anything else
  -- kind-specific so the schema doesn't need to grow per probe.
  url                  TEXT,
  hostname             TEXT,
  port                 INT,
  config               JSONB           NOT NULL DEFAULT '{}'::jsonb,
  -- Scheduling
  interval_seconds     INT             NOT NULL DEFAULT 60
                                       CHECK (interval_seconds BETWEEN 10 AND 86400),
  retry_interval_sec   INT             NOT NULL DEFAULT 60,
  max_retries          INT             NOT NULL DEFAULT 0,
  timeout_seconds      INT             NOT NULL DEFAULT 16,
  resend_interval_sec  INT             NOT NULL DEFAULT 0,   -- repeat alert every N sec while down (0 = once)
  upside_down          BOOLEAN         NOT NULL DEFAULT FALSE, -- inverts pass/fail (expects DOWN)
  -- HTTP common opts (also stored here so they're searchable / indexable)
  http_method          TEXT            NOT NULL DEFAULT 'GET',
  http_body            TEXT,
  http_headers         JSONB,                                 -- {"Authorization":"Bearer ..."}
  accepted_statuses    INT[]           NOT NULL DEFAULT
    ARRAY[200,201,202,203,204,205,206,207,208,226]::int[],
  follow_redirect      BOOLEAN         NOT NULL DEFAULT TRUE,
  ignore_tls           BOOLEAN         NOT NULL DEFAULT FALSE,
  proxy_id             UUID            REFERENCES proxies(id) ON DELETE SET NULL,
  -- State
  active               BOOLEAN         NOT NULL DEFAULT TRUE,
  current_status       monitor_status  NOT NULL DEFAULT 'pending',
  created_at           TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
  updated_at           TIMESTAMPTZ     NOT NULL DEFAULT NOW()
);
CREATE INDEX monitors_active_idx ON monitors (active) WHERE active;

-- ─── heartbeats (individual check results) ───────────────────────────────
-- High-volume time-series. Before launch, partition by RANGE (ts) with
-- monthly partitions managed by pg_partman; un-partitioned here to keep
-- the initial migration simple.

CREATE TABLE heartbeats (
  monitor_id    UUID            NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  ts            TIMESTAMPTZ     NOT NULL,
  status        monitor_status  NOT NULL,
  latency_ms    INT,
  status_code   INT,                                   -- HTTP if applicable
  msg           TEXT,                                  -- error or info
  retries       INT             NOT NULL DEFAULT 0,
  important     BOOLEAN         NOT NULL DEFAULT FALSE, -- status changed at this point
  PRIMARY KEY (monitor_id, ts)
);
CREATE INDEX heartbeats_monitor_ts_idx  ON heartbeats (monitor_id, ts DESC);
CREATE INDEX heartbeats_important_idx   ON heartbeats (monitor_id, ts DESC) WHERE important;

-- ─── tags (with optional values, e.g. env=prod) ───────────────────────────

CREATE TABLE tags (
  id        UUID         PRIMARY KEY,
  name      TEXT         UNIQUE NOT NULL,
  color     TEXT         NOT NULL DEFAULT '#14b8a6',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE monitor_tags (
  monitor_id  UUID NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  tag_id      UUID NOT NULL REFERENCES tags(id)     ON DELETE CASCADE,
  value       TEXT,                                  -- e.g. tag "env" value "prod"
  PRIMARY KEY (monitor_id, tag_id)
);

-- ─── notification templates ──────────────────────────────────────────────
-- Declared before notifications because notifications.template_id refs it.
-- Variables in the body are {{ handlebars-style }}: {{monitor.name}},
-- {{heartbeat.status}}, {{heartbeat.msg}}, etc. Render is shared across
-- channel kinds; subject is used by email + PagerDuty.

CREATE TABLE notification_templates (
  id                UUID         PRIMARY KEY,
  name              TEXT         UNIQUE NOT NULL,
  channel_kinds     TEXT[]       NOT NULL,
  event_kind        TEXT         NOT NULL CHECK (event_kind IN (
    'monitor_down','monitor_up','monitor_warn',
    'maintenance_start','maintenance_end',
    'incident_created','incident_updated','incident_resolved'
  )),
  subject_template  TEXT,
  body_template     TEXT         NOT NULL,
  is_default        BOOLEAN      NOT NULL DEFAULT FALSE,
  created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
CREATE INDEX notification_templates_default_idx
  ON notification_templates (event_kind) WHERE is_default;

-- ─── notification channels ───────────────────────────────────────────────
-- Channels are first-class. Per-monitor wiring lives in monitor_notifications.

CREATE TYPE channel_kind AS ENUM (
  'slack','discord','telegram','email','webhook','pagerduty',
  'sms_twilio','teams','gotify','pushover','ntfy','mattermost',
  'matrix','signal','rocket_chat','google_chat','zulip','line',
  'lark','wecom','dingtalk','feishu','custom'
);

CREATE TABLE notifications (
  id              UUID            PRIMARY KEY,
  name            TEXT            NOT NULL,
  kind            channel_kind    NOT NULL,
  config          JSONB           NOT NULL,                       -- secrets encrypted app-side
  is_default      BOOLEAN         NOT NULL DEFAULT FALSE,
  template_id     UUID            REFERENCES notification_templates(id) ON DELETE SET NULL,
  active          BOOLEAN         NOT NULL DEFAULT TRUE,
  last_fired_at   TIMESTAMPTZ,
  created_at      TIMESTAMPTZ     NOT NULL DEFAULT NOW()
);

CREATE TABLE monitor_notifications (
  monitor_id      UUID NOT NULL REFERENCES monitors(id)      ON DELETE CASCADE,
  notification_id UUID NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
  PRIMARY KEY (monitor_id, notification_id)
);

-- ─── maintenance ──────────────────────────────────────────────────────────
-- single / recurring (interval, weekday, monthly) / cron strategies for
-- power users. During a window the targeted monitors keep checking (we
-- want the data) but don't fire notifications and render as 'maintenance'
-- on status pages.

CREATE TYPE maintenance_strategy AS ENUM (
  'manual',
  'single',
  'recurring_interval',
  'recurring_weekday',
  'recurring_monthly',
  'cron'
);

CREATE TABLE maintenance (
  id            UUID                  PRIMARY KEY,
  title         TEXT                  NOT NULL,
  description   TEXT,
  strategy      maintenance_strategy  NOT NULL,
  active        BOOLEAN               NOT NULL DEFAULT TRUE,
  -- 'single' uses starts_at + ends_at directly.
  -- 'recurring_*' use starts_at + duration_min + interval/weekday/monthly cols.
  -- 'cron'  uses cron_expr + duration_min.
  starts_at     TIMESTAMPTZ,
  ends_at       TIMESTAMPTZ,
  duration_min  INT,
  interval_days INT,                                     -- recurring_interval
  weekdays      INT[],                                   -- recurring_weekday (0=Sun..6=Sat)
  day_of_month  INT,                                     -- recurring_monthly (1..31)
  cron_expr     TEXT,                                    -- cron
  timezone      TEXT                  NOT NULL DEFAULT 'UTC',
  created_at    TIMESTAMPTZ           NOT NULL DEFAULT NOW(),
  created_by    UUID                  REFERENCES users(id)
);

CREATE TABLE monitor_maintenance (
  monitor_id      UUID NOT NULL REFERENCES monitors(id)    ON DELETE CASCADE,
  maintenance_id  UUID NOT NULL REFERENCES maintenance(id) ON DELETE CASCADE,
  PRIMARY KEY (monitor_id, maintenance_id)
);

-- ─── status pages ─────────────────────────────────────────────────────────

CREATE TABLE status_pages (
  id                   UUID         PRIMARY KEY,
  slug                 TEXT         UNIQUE NOT NULL CHECK (slug ~ '^[a-z0-9-]{2,40}$'),
  title                TEXT         NOT NULL,
  subtitle             TEXT,
  description          TEXT,
  icon                 TEXT,                                   -- emoji or URL
  theme                TEXT         NOT NULL DEFAULT 'auto' CHECK (theme IN ('light','dark','auto')),
  custom_css           TEXT,
  custom_domain        TEXT         UNIQUE,
  domain_verified      BOOLEAN      NOT NULL DEFAULT FALSE,
  google_analytics_id  TEXT,
  show_powered_by      BOOLEAN      NOT NULL DEFAULT TRUE,
  auto_degrade         BOOLEAN      NOT NULL DEFAULT TRUE,    -- auto-flip page status on incident
  password_hash        TEXT,                                   -- optional gate
  published            BOOLEAN      NOT NULL DEFAULT FALSE,
  created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE status_page_groups (
  id              UUID         PRIMARY KEY,
  status_page_id  UUID         NOT NULL REFERENCES status_pages(id) ON DELETE CASCADE,
  name            TEXT         NOT NULL,
  sort_order      INT          NOT NULL DEFAULT 0
);
CREATE INDEX status_page_groups_page_idx ON status_page_groups (status_page_id, sort_order);

CREATE TABLE status_page_components (
  id              UUID         PRIMARY KEY,
  group_id        UUID         NOT NULL REFERENCES status_page_groups(id) ON DELETE CASCADE,
  monitor_id      UUID         NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  display_name    TEXT,                                       -- override monitor.name
  sort_order      INT          NOT NULL DEFAULT 0
);

CREATE TABLE status_page_subscribers (
  id              UUID         PRIMARY KEY,
  status_page_id  UUID         NOT NULL REFERENCES status_pages(id) ON DELETE CASCADE,
  channel         TEXT         NOT NULL CHECK (channel IN ('email','sms','webhook','rss','slack')),
  destination     TEXT         NOT NULL,
  confirmed       BOOLEAN      NOT NULL DEFAULT FALSE,
  confirm_token   TEXT,
  subscribed_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- ─── incidents (status-page announcements) ────────────────────────────────
-- Massively simpler than Rampart v1's design. An incident here is a
-- message posted to a status page, with optional running updates. No
-- severity beyond a visual `style`; no root cause, no AI summary, no
-- post-mortem. Just communication.

CREATE TYPE incident_style AS ENUM ('info','warning','danger','primary','success');

CREATE TABLE incidents (
  id              UUID            PRIMARY KEY,
  status_page_id  UUID            NOT NULL REFERENCES status_pages(id) ON DELETE CASCADE,
  title           TEXT            NOT NULL,
  content         TEXT            NOT NULL,
  style           incident_style  NOT NULL DEFAULT 'warning',
  pinned          BOOLEAN         NOT NULL DEFAULT TRUE,
  active          BOOLEAN         NOT NULL DEFAULT TRUE,
  resolved_at     TIMESTAMPTZ,
  created_at      TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
  created_by      UUID            REFERENCES users(id)
);
CREATE INDEX incidents_active_idx ON incidents (status_page_id) WHERE active;

CREATE TABLE incident_updates (
  id            UUID         PRIMARY KEY,
  incident_id   UUID         NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
  message       TEXT         NOT NULL,
  posted_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  posted_by     UUID         REFERENCES users(id)
);
CREATE INDEX incident_updates_idx ON incident_updates (incident_id, posted_at);

-- ─── API keys ─────────────────────────────────────────────────────────────

CREATE TABLE api_keys (
  id            UUID         PRIMARY KEY,
  name          TEXT         NOT NULL,
  key_hash      TEXT         UNIQUE NOT NULL,            -- argon2
  key_prefix    TEXT         NOT NULL,                   -- first 12 chars for display
  scopes        TEXT[]       NOT NULL,
  created_by    UUID         REFERENCES users(id),
  created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  last_used_at  TIMESTAMPTZ,
  expires_at    TIMESTAMPTZ
);

-- ─── status badges ───────────────────────────────────────────────────────

CREATE TABLE status_badges (
  id              UUID         PRIMARY KEY,
  monitor_id      UUID         REFERENCES monitors(id) ON DELETE CASCADE,
  status_page_id  UUID         REFERENCES status_pages(id) ON DELETE CASCADE,
  label           TEXT         NOT NULL DEFAULT 'status',
  style           TEXT         NOT NULL DEFAULT 'flat'
                               CHECK (style IN ('flat','flat-square','for-the-badge','plastic')),
  logo_svg        TEXT,
  custom_colors   JSONB        NOT NULL DEFAULT '{}'::jsonb,
  CHECK ((monitor_id IS NOT NULL) <> (status_page_id IS NOT NULL))
);

-- ─── workspace settings (key-value) ───────────────────────────────────────
-- For SMTP creds, branding, retention policies, etc. JSONB so we can grow
-- the schema without migrations.

CREATE TABLE settings (
  key         TEXT         PRIMARY KEY,
  value       JSONB        NOT NULL,
  updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- ─── audit log ────────────────────────────────────────────────────────────

CREATE TABLE audit_log (
  id                BIGSERIAL    PRIMARY KEY,
  actor_user_id     UUID         REFERENCES users(id),
  actor_api_key_id  UUID         REFERENCES api_keys(id),
  action            TEXT         NOT NULL,    -- 'monitor.create', 'incident.resolve', ...
  resource_kind     TEXT         NOT NULL,
  resource_id       UUID,
  payload           JSONB,
  ip_addr           INET,
  user_agent        TEXT,
  ts                TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
CREATE INDEX audit_log_ts_idx ON audit_log (ts DESC);

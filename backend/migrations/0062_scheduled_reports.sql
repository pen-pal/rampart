-- Scheduled uptime reports.
--
-- A small admin-managed table driving the scheduler's weekly uptime
-- digest. Each row is one recurring report: a named set of email
-- recipients that receives a per-monitor 7-day uptime summary on a
-- cadence. The scheduler runs a best-effort slow-tick check that, for any
-- row whose `last_sent_at` is NULL or older than 7 days, renders the
-- digest and sends it via the existing system SMTP sender (the same path
-- maintenance-subscriber emails use), then stamps `last_sent_at = NOW()`.
--
--   recipients    TEXT[] of email addresses. Empty is allowed but the
--                 scheduler simply skips a row with no recipients.
--   cadence       TEXT, currently only 'weekly'. Kept as free text rather
--                 than an enum so adding 'daily'/'monthly' later needs no
--                 migration; the scheduler treats anything it doesn't
--                 recognise as weekly.
--   last_sent_at  NULL until the first successful send. The 7-day gate is
--                 computed against this, so a brand-new report fires on the
--                 next slow tick.

CREATE TABLE IF NOT EXISTS scheduled_reports (
    id           UUID PRIMARY KEY,
    name         TEXT        NOT NULL,
    recipients   TEXT[]      NOT NULL DEFAULT '{}',
    cadence      TEXT        NOT NULL DEFAULT 'weekly',
    last_sent_at TIMESTAMPTZ NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

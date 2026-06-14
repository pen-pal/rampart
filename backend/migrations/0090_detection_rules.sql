-- SIEM-style log detection rules + their findings. See
-- docs/design/DETECTION.md.
--
-- A detection rule is a saved query over the `logs` tier (service scope +
-- minimum severity + optional case-insensitive body regex). Each scheduler
-- tick counts the NEW matching rows since `last_checked_at`; when that count
-- reaches `threshold` the rule raises a `detection_findings` row and notifies
-- its channels. The watermark makes evaluation restart-safe and prevents
-- double-counting across ticks. The tables are inert until an operator creates
-- a rule — no rules, no work.
CREATE TABLE detection_rules (
    id              UUID        PRIMARY KEY,
    name            TEXT        NOT NULL,
    description     TEXT        NOT NULL DEFAULT '',
    enabled         BOOLEAN     NOT NULL DEFAULT true,
    severity        TEXT        NOT NULL DEFAULT 'medium',
    -- match spec over logs; empty string / 0 = "no constraint on this field".
    service         TEXT        NOT NULL DEFAULT '',
    min_level       SMALLINT    NOT NULL DEFAULT 0,
    body_regex      TEXT        NOT NULL DEFAULT '',
    -- raise a finding when >= threshold new matches land in a tick.
    threshold       INT         NOT NULL DEFAULT 1,
    -- lookback used only when the rule has never run (no watermark yet).
    window_seconds  INT         NOT NULL DEFAULT 300,
    channel_ids     UUID[]      NOT NULL DEFAULT '{}',
    -- watermark: upper bound of the last evaluated window (ts of newest row
    -- considered). NULL = never evaluated.
    last_checked_at TIMESTAMPTZ NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT detection_rules_severity_check
        CHECK (severity IN ('low', 'medium', 'high', 'critical'))
);

CREATE TABLE detection_findings (
    id              UUID        PRIMARY KEY,
    rule_id         UUID        NOT NULL REFERENCES detection_rules(id) ON DELETE CASCADE,
    -- denormalised so findings remain readable after a rule is renamed/deleted
    -- (the FK cascade deletes findings, but rule_name keeps logs/exports sane).
    rule_name       TEXT        NOT NULL,
    severity        TEXT        NOT NULL,
    match_count     BIGINT      NOT NULL,
    sample          TEXT        NULL,
    service         TEXT        NULL,
    window_from     TIMESTAMPTZ NOT NULL,
    window_to       TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    acknowledged_at TIMESTAMPTZ NULL,
    CONSTRAINT detection_findings_severity_check
        CHECK (severity IN ('low', 'medium', 'high', 'critical'))
);

-- Findings feed: newest first, with a fast "unacknowledged only" path.
CREATE INDEX detection_findings_recent_idx ON detection_findings (created_at DESC);
CREATE INDEX detection_findings_open_idx ON detection_findings (created_at DESC)
    WHERE acknowledged_at IS NULL;
CREATE INDEX detection_findings_rule_idx ON detection_findings (rule_id, created_at DESC);

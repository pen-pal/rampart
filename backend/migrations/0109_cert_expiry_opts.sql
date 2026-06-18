-- Opt-in TLS certificate-expiry checking on http/https monitors.
--
-- Today the scheduler RECORDS a cert snapshot (cert_days_left / cert_subject /
-- cert_checked_at) for https HTTP-family monitors, but a near-expiry or expired
-- cert never changes the monitor's status. These two columns let an operator
-- opt a single http/https monitor into cert-expiry status evaluation alongside
-- the regular HTTP check:
--
--   * check_cert       — when true, an http(s) monitor ALSO evaluates the TLS
--                        cert: expired/invalid -> Down, near-expiry -> Warn.
--                        Default false so every existing monitor is unchanged.
--                        For tls-kind monitors the cert IS the check, so this
--                        flag is implicitly true and not required.
--   * cert_expiry_days — warn threshold in days: Warn when days-left <= this.
--                        Default 14, matching the tls probe's historic
--                        config.warn_days default.

ALTER TABLE monitors
    ADD COLUMN IF NOT EXISTS check_cert       BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS cert_expiry_days INT     NOT NULL DEFAULT 14;

-- Allow the RUM Largest-Contentful-Paint p75 alert kind on telemetry rules.
-- Fires when real-user load performance regresses past a threshold (ms).
ALTER TABLE telemetry_alert_rules DROP CONSTRAINT telemetry_alert_rules_kind_check;
ALTER TABLE telemetry_alert_rules ADD CONSTRAINT telemetry_alert_rules_kind_check
    CHECK (kind IN ('error_rate', 'trace_latency', 'trace_error_rate', 'log_volume',
                    'profile_samples', 'rum_lcp_p75'));

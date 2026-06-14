-- Allow the profiling-volume alert kind on telemetry_alert_rules. SUM of
-- profile sample_count over a window (rampart_db::telemetry_rules::observe).
ALTER TABLE telemetry_alert_rules DROP CONSTRAINT telemetry_alert_rules_kind_check;
ALTER TABLE telemetry_alert_rules ADD CONSTRAINT telemetry_alert_rules_kind_check
    CHECK (kind IN ('error_rate', 'trace_latency', 'trace_error_rate', 'log_volume', 'profile_samples'));

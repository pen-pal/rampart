-- Allow the z-score anomaly op on metric_rules. The evaluator compares the
-- latest sample to a rolling mean/stddev baseline; threshold = sensitivity in σ.
ALTER TABLE metric_rules DROP CONSTRAINT metric_rules_op_check;
ALTER TABLE metric_rules ADD CONSTRAINT metric_rules_op_check
    CHECK (op IN ('gt', 'lt', 'gte', 'lte', 'anomaly'));

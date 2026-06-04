-- SNMP v2c GET probe kind (wire-compatible with SNMPv1 GET).
-- Issues a single SNMP GET for the configured OID against a
-- UDP-reachable agent; Up when the agent replies with a varbind.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'snmp';

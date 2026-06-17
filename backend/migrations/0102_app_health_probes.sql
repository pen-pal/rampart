-- App-health probe kinds: Elasticsearch/OpenSearch, Vault, etcd.
-- HTTP-based health checks for common platform/SIEM infrastructure. Kind config
-- (allow_yellow, username/password) lives in the freeform monitor.config JSONB.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'elasticsearch';
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'vault';
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'etcd';

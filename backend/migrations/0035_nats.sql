-- NATS probe kind.
-- Connects to a `nats://host:port` server, runs the INFO / CONNECT /
-- PING handshake via the async-nats client, asserts a successful flush.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'nats';

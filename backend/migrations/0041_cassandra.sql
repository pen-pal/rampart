-- Cassandra / ScyllaDB probe kind.
-- Opens a CQL session to the listed node, runs `SELECT release_version
-- FROM system.local` to confirm the server is past the CQL handshake
-- and answering queries.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'cassandra';

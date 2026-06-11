-- Remote probe agents (batch-16).
--
-- An agent is a lightweight `rampart-agent` worker running somewhere else
-- (another region, a private network segment) that probes its assigned
-- monitors and reports heartbeats back over the HTTP API. The agent dials
-- OUT to the server — the server never connects to the agent — so an agent
-- behind NAT / a corporate firewall works with no inbound holes.
--
-- Auth mirrors api_keys: only the SHA-256 hash of the bearer secret
-- (`rmpa_<40 chars>`) is stored; the raw value is shown once at creation.
--
-- `last_seen_at` bumps on every pull/report and drives the dashboard's
-- online/offline badge plus the scheduler's stale-agent watchdog.
CREATE TABLE agents (
    id           UUID PRIMARY KEY,
    name         TEXT        NOT NULL,
    -- Free-form location label ("eu-west · Hetzner FSN"). Cosmetic; shown
    -- on the dashboard and in heartbeat messages.
    location     TEXT        NULL,
    token_hash   TEXT        NOT NULL UNIQUE,
    -- Agent build version, self-reported on each poll. Lets the operator
    -- spot agents that lag behind the server after an upgrade.
    version      TEXT        NULL,
    last_seen_at TIMESTAMPTZ NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- NULL = probed locally by the in-process scheduler (every existing
-- monitor keeps its current behaviour). Set = probed by that agent; the
-- local scheduler skips it. ON DELETE SET NULL so revoking an agent
-- returns its monitors to local probing instead of orphaning them.
ALTER TABLE monitors
    ADD COLUMN agent_id UUID NULL REFERENCES agents(id) ON DELETE SET NULL;

-- The agent pull endpoint and the scheduler's "skip remote" filter both
-- select on agent_id; partial index keeps it free for the common
-- (local-only) install.
CREATE INDEX monitors_agent_idx ON monitors (agent_id) WHERE agent_id IS NOT NULL;

#!/bin/sh
# The agent's rmpa_ token is minted by `provision` (POST /v1/agents) and written
# to the shared volume. Wait for it, source it, then run the agent. It pulls its
# assigned monitor (a private-only target reachable only from inside the network)
# and reports heartbeats back — proving the remote-probe path end to end.
set -e
echo "[agent] waiting for the agent token from provision…"
i=0
while [ ! -f /shared/agent.env ]; do
  i=$((i + 1)); [ "$i" -gt 120 ] && echo "[agent] gave up waiting for /shared/agent.env" && exit 1
  sleep 2
done
# shellcheck disable=SC1091
. /shared/agent.env
export RAMPART_URL RAMPART_AGENT_TOKEN
echo "[agent] starting against $RAMPART_URL"
exec /usr/local/bin/rampart-agent

# syntax=docker/dockerfile:1.7
# Build the remote probe agent FROM SOURCE. The published rampart image omits
# the agent binary, so this stack always compiles it. Build context is the repo
# root (../../.. from this file) so the workspace + crates are available.
#
#   docker compose build rampart-agent
#
FROM rust:1.96-slim-bookworm AS build
WORKDIR /src
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY backend/ ./backend/
WORKDIR /src/backend
ENV SQLX_OFFLINE=true CARGO_TERM_COLOR=never
RUN cargo build --release -p rampart-agent
RUN strip target/release/rampart-agent

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends libssl3 ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --no-create-home --shell /usr/sbin/nologin rampart
COPY --from=build /src/backend/target/release/rampart-agent /usr/local/bin/rampart-agent
COPY examples/everything/agent/agent-entrypoint.sh /agent-entrypoint.sh
RUN chmod +x /agent-entrypoint.sh
USER rampart
ENTRYPOINT ["/usr/bin/tini", "--", "/agent-entrypoint.sh"]

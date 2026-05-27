# syntax=docker/dockerfile:1.7

# ─── stage 1: build the React bundle ─────────────────────────────────────
FROM node:20-alpine AS frontend
WORKDIR /src
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm ci
COPY frontend/ ./
RUN npm run build


# ─── stage 2: build the Rust binary ──────────────────────────────────────
# rust-embed inlines frontend/dist/ into the binary at compile time. The
# embed macro reads "../../../frontend/dist/" relative to rampart-api, so
# we materialize the same layout inside the build context.
FROM rust:1.82-slim-bookworm AS backend
WORKDIR /src

# Build-time C toolchain + pkg-config for the few native deps in the tree
# (e.g. ring used by rustls).
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Copy workspace + embedded frontend so the macro path resolves.
COPY backend/         ./backend/
COPY --from=frontend  /src/dist/  ./frontend/dist/

WORKDIR /src/backend
ENV SQLX_OFFLINE=true CARGO_TERM_COLOR=never
RUN cargo build --release -p rampart-api
RUN strip target/release/rampart-api


# ─── stage 3: minimal runtime ────────────────────────────────────────────
# debian-slim, not distroless: we need libssl + ca-certs at runtime for
# the long tail of HTTP clients in rampart-notifier.
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        libssl3 \
        ca-certificates \
        tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --no-create-home --shell /usr/sbin/nologin rampart

COPY --from=backend  /src/backend/target/release/rampart-api  /usr/local/bin/rampart-api
COPY backend/migrations/                                       /opt/rampart/migrations/

USER rampart
EXPOSE 3000

ENV BIND_ADDR=0.0.0.0:3000 \
    RUST_LOG=rampart=info,tower_http=warn,info

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/rampart-api"]

# Deployment artifacts

Production-grade scaffolding for running Rampart on a self-hosted box. Everything in this directory is opt-in — `docker compose up` from the repo root still works as the zero-config path.

## What's here

| File                  | Purpose                                                                                                  |
| :-------------------- | :------------------------------------------------------------------------------------------------------- |
| `rampart.service`     | systemd unit. Wraps `docker compose up -d --wait` + `down`. Survives docker daemon restarts.             |
| `backup-postgres.sh`  | Rotating `pg_dump` of the Postgres volume, custom format, default 14-day retention. Cron-friendly.       |

## Install the systemd unit

```bash
# 1. Clone Rampart to /opt/rampart (or symlink it there)
sudo git clone https://github.com/pen-pal/rampart /opt/rampart

# 2. Drop the unit in place + enable it
sudo cp /opt/rampart/docs/deploy/rampart.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now rampart

# 3. Watch the logs
sudo journalctl -u rampart -f
```

The unit's `Type=oneshot` + `RemainAfterExit=yes` pattern is the right shape for a compose wrapper: the unit goes "active" once `docker compose up -d --wait` returns (which only happens after the healthchecks in `compose.yaml` report healthy), then stays active until the operator runs `systemctl stop rampart`.

## Back up Postgres

```bash
# One-shot test run
sudo /opt/rampart/docs/deploy/backup-postgres.sh

# Cron it for daily 03:14 UTC
sudo crontab -e
# Then add:
14 3 * * *  /opt/rampart/docs/deploy/backup-postgres.sh
```

Output lands in `/var/backups/rampart/rampart-<utc-iso>.dump` (chmod 600). Pruned automatically on each run via `BACKUP_KEEP_DAYS` (default 14).

## Restore from a dump

```bash
# Stop Rampart so no writes race the restore
sudo systemctl stop rampart

# Drop + recreate the database, then restore
docker exec -i rampart-postgres-1 dropdb -U rampart rampart
docker exec -i rampart-postgres-1 createdb -U rampart rampart
docker exec -i rampart-postgres-1 pg_restore -U rampart -d rampart -1 \
  < /var/backups/rampart/rampart-2026-06-09T03-14-00Z.dump

sudo systemctl start rampart
```

`pg_restore -1` runs the whole restore in a single transaction so a failure mid-restore rolls back cleanly without leaving the DB half-populated.

## Compose hardening

`compose.yaml` at the repo root applies the following production defaults out of the box:

- Resource limits (Rampart: 512 MiB / 1 CPU, Postgres: 1 GiB / 1 CPU). Override per-host via the `deploy.resources` block.
- `restart: unless-stopped` for both services.
- Healthchecks on both services; the Rampart container's check hits `/readyz` which only returns 200 when the DB is reachable.
- `read_only: true` rootfs on the Rampart container with a `tmpfs:/tmp` mount for the bits that need write access.
- `cap_drop: ALL` + `cap_add: NET_RAW` (only the cap the ICMP-ping probe needs).
- `security_opt: no-new-privileges:true` on both services.
- `stop_signal: SIGTERM` + `stop_grace_period: 30s` so the binary's graceful-shutdown handler completes before the container is killed.
- `RAMPART_LOG_FORMAT=json` so logs ship to aggregators as structured records with `request_id` as a first-class field.

Override any of these via a `compose.override.yaml` next to the main file — Docker Compose merges them automatically.

## Reverse proxy

Rampart binds plain HTTP on the configured port. Put a reverse proxy in front for TLS termination + HTTP/2 + (optionally) extra request limits and CORS rules. A minimal nginx fragment:

```nginx
location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_http_version 1.1;
    proxy_set_header Host              $host;
    proxy_set_header X-Real-IP         $remote_addr;
    proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;

    # SSE — disable buffering so the live-heartbeat stream flushes
    # events to the browser as they arrive.
    proxy_buffering off;
    proxy_read_timeout 1d;
}
```

Caddy is similarly one-liner:

```caddy
rampart.example.com {
    reverse_proxy 127.0.0.1:3000 {
        flush_interval -1
    }
}
```

## What this directory does NOT cover

- **Kubernetes** — Rampart is single-tenant and stateful. Helm chart contributions welcome (see CONTRIBUTING.md `In Scope`).
- **Multi-region replication** — out of scope for the project (see `docs/DESIGN-ORIGINAL.md`). Run one box per region if you need that.
- **Postgres tuning** — defaults from the postgres-alpine image are fine through ~1000 monitors. Past that, drop a `postgresql.conf` mount in and tune `shared_buffers` / `work_mem` / `max_connections` to your hardware.

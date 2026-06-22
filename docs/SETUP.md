# Setup

Get Rampart running end-to-end in under five minutes. Three paths,
pick the one that matches your situation.

---

## Path A · Docker compose (recommended)

Easiest. One command builds the image + brings up Postgres + Rampart.

### 1. Clone

```bash
git clone https://github.com/pen-pal/rampart.git
cd rampart
```

### 2. (Optional) Override credentials

The defaults work for a local dev box. For a real deployment, copy the
template + edit:

```bash
cp .env.example .env 2>/dev/null || true
cat > .env <<'EOF'
POSTGRES_USER=rampart
POSTGRES_PASSWORD=change-me-in-prod
POSTGRES_DB=rampart
RAMPART_PORT=3000
EOF
```

### 3. Bring up the stack

```bash
docker compose up -d --build
docker compose logs -f rampart        # watch boot logs
```

The first `up` builds the Rust binary, embeds the React bundle, and
starts both Postgres + Rampart. Migrations run automatically on
boot — there is no separate migrate step.

### 4. First-run admin

Open <http://localhost:3000>. The first visit shows a signup form
(only renders when zero users exist). The first user becomes admin.
Subsequent users come through the admin **Users** page; the signup
form locks itself after the first account.

**Locked out, or scripting a fresh install?** Create or reset an admin from the
binary — resets the password if the email exists, else creates an admin (any
password, no policy gate):

```bash
docker compose exec rampart rampart-api reset-password admin@example.com 'your-password'
# single binary:  ./rampart-api reset-password admin@example.com 'your-password'
```

### 4b. (Optional) Load demo data

Want to see the whole dashboard populated before wiring anything up? The
binary ships a `seed-demo` subcommand that inserts one representative slice of
every tier — monitors with 48h of uptime history (one with an outage), a
folder, a notification channel, an error project with grouped issues +
breadcrumbs, a multi-service trace, logs, RUM web-vitals, a metric series, a
telemetry alert rule, and a SIEM detection rule that immediately raises a
finding. It is **idempotent** (re-running is a no-op) and everything is tagged
`[demo]` so you can spot and delete it later.

```bash
# Docker compose:
docker compose exec rampart rampart-api seed-demo

# Single binary:
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./rampart-api seed-demo
```

Refresh the dashboard — monitors, errors, traces, logs, RUM, detection
findings and an alert rule are all there. See [DEMO.md](DEMO.md).

### 5. Add a monitor

`+ Add monitor` → pick a kind → fill in fields → save. Heartbeats land
on the next tick. Default interval is 60 s; the dashboard polls every
10 s so flips show up within ~70 s end-to-end.

### 6. Wire alerts

`#/notifications` → `+ Add a new channel` → pick from 128 channels →
test → save. Attach to monitors from the monitor detail page sidebar.
For Web Push, save the channel first, then click **Enable push** on its
row to subscribe the browser.

### 7. (Optional) Add 2FA

`#/security` → "Set up authenticator" → scan QR → enter code → save
the recovery codes somewhere safe.

### 8. (Optional) Status page

`#/status-page` → `+ New status page` → pick a slug → attach monitors →
save. Public URL: `<rampart>/#/s/<slug>`. Subscribers can opt-in to
incident emails (configure SMTP at `#/settings/smtp`).

---

## Path B · Single binary (no Docker)

Useful for bare-metal homelabs or systemd-managed services.

### Prerequisites

| Tool       | Min version | Notes                                     |
| ---        | ---         | ---                                       |
| Postgres   | 14          | 16+ recommended                           |
| Rust       | 1.88        | https://rustup.rs (transitive deps need edition2024) |
| Node       | 20          | for the frontend bundle                   |
| `sqlx-cli` | 0.8         | only needed if you want to inspect migrations |

### Steps

```bash
# 1. Run Postgres any way you like; the app needs a DATABASE_URL.
#    The repo ships a dev compose:
cd backend && docker compose up -d postgres && cd ..

# 2. Build the SPA.
cd frontend && npm ci && npm run build && cd ..

# 3. Build the Rust binary. `rust-embed` reads frontend/dist/ at
#    compile time, so make sure step 2 has finished first.
cd backend && cargo build --release -p rampart-api && cd ..

# 4. Run it. Migrations run on boot.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
BIND_ADDR=0.0.0.0:3000 \
RUST_LOG=rampart=info,tower_http=warn,info \
  ./backend/target/release/rampart-api
```

The binary is self-contained (~10 MB stripped). For systemd, drop it in
`/usr/local/bin/rampart-api` and write a one-screen unit file that
sets `DATABASE_URL` and runs the binary.

---

## Path C · Dev mode (hot reload)

For working on Rampart itself.

```bash
# Terminal 1 — Postgres
cd backend && docker compose up -d postgres

# Terminal 2 — backend
cd backend && cargo run -p rampart-api
# Debug builds read frontend/dist/ from disk, so frontend rebuilds
# pick up without a backend rebuild.

# Terminal 3 — frontend with HMR
cd frontend && npm run dev
# Open http://localhost:5173 — vite proxies /v1, /push, /healthz to :3000.
```

---

## Production checklist

Before exposing to the internet:

1. **Reverse-proxy with TLS.** Bind Rampart to `127.0.0.1:3000` and
   put nginx / caddy / traefik in front:
   ```
   BIND_ADDR=127.0.0.1:3000
   ```
   Set `X-Forwarded-Proto: https` from the proxy so session cookies
   are marked `Secure` automatically.

2. **Strong Postgres password.** Change `POSTGRES_PASSWORD` in `.env`
   before first boot; if you already booted with the default, recreate
   the volume:
   ```
   docker compose down -v
   ```

3. **Backups.** `pg_dump` the `rampart` database on a schedule. Cert
   snapshots, heartbeats, and tokens all live there.

4. **SMTP for subscribers.** `#/settings/smtp` (admin). Without this,
   incident emails are silently dropped (logged but not failed).

5. **Enable 2FA on the admin account.** `#/security`. Print the
   recovery codes and store them offline.

6. **Tighten CORS.** `rampart-api`'s default allows any origin. For
   production, edit `crates/rampart-api/src/lib.rs::build_router` to
   restrict `CorsLayer` to your domain.

7. **Audit log retention.** `audit_log` grows unbounded; add a cron
   `DELETE FROM audit_log WHERE ts < NOW() - INTERVAL '90 days'` if
   long-term records aren't a compliance requirement.

8. **Heartbeat retention.** Same story — `heartbeats` grows
   unbounded today. Plan a pruning job until partition rotation lands.

9. **(Optional) Row-Level Security (`RAMPART_RLS`).** Postgres RLS is a
   *defense-in-depth* layer for multi-tenancy — the app already scopes every
   query by `org_id` and is tested; RLS is belt-and-suspenders so a missed
   filter can't leak across orgs. It is **off by default** and fully
   reversible. To enable it:

   Set `RAMPART_RLS=1` (or `true`/`yes`) and restart. The pool then binds each
   request's org onto its connection (`SET ROLE rampart_app` + the
   `app.current_org` GUC), and the policies — `ENABLE`d by migration 0116 —
   enforce per-org isolation on those tenant connections.

   **No `BYPASSRLS` grant is required** for the standard single-role
   deployment. RLS is `ENABLE`d but **not** `FORCE`d, so the table *owner* (the
   role that ran the migrations — your `DATABASE_URL` user) is exempt by
   ownership. Therefore:
   - with `RAMPART_RLS` off, every checkout stays the owner → nothing is
     enforced → byte-identical to before;
   - with it on, only tenant requests `SET ROLE rampart_app` (enforced); the
     system loops (scheduler/prune/notifier/SIEM/self-metrics/migrate/import/
     leader) bind no org, stay the owner, and bypass for free.

   Only if your app connects as a role that does **not** own the tables
   (uncommon — e.g. migrations applied by a separate admin role) must you grant
   the runtime role `BYPASSRLS` (`ALTER ROLE <db_user> BYPASSRLS;`) so its
   owner-less checkouts aren't policy-subject. Validate isolation on a shadow
   database before enabling in production; `RAMPART_RLS=0` fully reverts
   enforcement with no schema change.

---

## SSO (OIDC)

Put Rampart behind your identity provider (Google, Okta, Keycloak, Authentik,
Entra, …) instead of local passwords. Generic OpenID Connect — Rampart reads the
provider's discovery document and drives the **Authorization Code flow with
PKCE**. Enabled when all four required vars are set:

| Env var | Required | Notes |
| --- | --- | --- |
| `RAMPART_OIDC_ISSUER` | yes | e.g. `https://accounts.google.com` (no trailing slash needed) |
| `RAMPART_OIDC_CLIENT_ID` | yes | OAuth client id |
| `RAMPART_OIDC_CLIENT_SECRET` | yes | confidential client secret |
| `RAMPART_OIDC_REDIRECT_URL` | yes | e.g. `https://rampart.example.com/v1/auth/oidc/callback` |
| `RAMPART_OIDC_DEFAULT_ROLE` | no | `admin`\|`editor`\|`readonly` (default `readonly`; the **first** user provisioned becomes `admin`) |
| `RAMPART_OIDC_ORG_CLAIM` | no | a token/userinfo claim (`groups`, a custom `org`, Google's `hd`) mapping the identity to org(s) by slug |

Register `…/v1/auth/oidc/callback` as the redirect URI at your IdP. A login
button appears on the sign-in page once enabled.

**Security (issue #13 hardening):**

- **`id_token` is signature-verified.** Rampart fetches the issuer's JWKS and
  cryptographically verifies the `id_token` (signature + `iss`/`aud`/`exp`/`nbf`
  + a per-login `nonce`). The header algorithm is allow-listed to **asymmetric**
  algorithms only (RS/PS/ES/EdDSA) and a symmetric (HMAC/`oct`) JWK is rejected,
  defeating algorithm-confusion attacks. Identity is taken from the verified
  `id_token` claims, falling back to the userinfo endpoint only for claims the
  `id_token` omits. An `email_verified=true` assertion is still required before
  provisioning or linking an account. The JWKS is cached (1h) with a 60-second
  refetch cooldown so a flood of bogus-`kid` tokens can't amplify JWKS fetches.

- **All OIDC outbound traffic is SSRF-guarded.** Discovery, token, userinfo, and
  JWKS requests go through the same SSRF guard as probes/webhooks — the cloud
  metadata endpoint (`169.254.169.254`), loopback, and link-local addresses are
  always blocked, on every request and redirect hop.

- **`RAMPART_SSRF_BLOCK_PRIVATE` interaction.** If you turn on the private-range
  block (recommended for internet-exposed multi-user installs), a **private
  self-hosted IdP** (Keycloak/Authentik on a `10.x` / `192.168.x` / ULA address)
  is **auto-allowed**: the host parsed from `RAMPART_OIDC_ISSUER` is added to a
  narrow allow-list that exempts it from the private-range block only. The
  always-blocked set (metadata/loopback/link-local) stays blocked even for the
  issuer host, so this never re-opens an SSRF hole.

- **Login is rate-limited.** `/v1/auth/oidc/*` sits under the per-IP auth
  rate-limiter (10 burst, ~10/min/IP) since `/login` writes a pre-auth state row
  per hit.

---

## Common issues

| Symptom                                  | Cause / fix                                         |
| ---                                      | ---                                                 |
| `connection refused` on first run        | Postgres not ready yet; `docker compose ps` and wait for `healthy` |
| Ping monitor fails with `EACCES`         | Linux needs `CAP_NET_RAW` or `sysctl net.ipv4.ping_group_range="0 2147483647"`; compose grants it via `cap_add: NET_RAW` |
| TLS cert card empty on HTTPS monitor     | First inspection runs ~one tick after the monitor starts; appears within an interval + hourly cache TTL |
| Push monitor stuck in Pending            | URL is `<rampart>/push/<token>?status=up`. Token visible on the monitor's overview card |
| Login form rejects with "wrong password" | Could be the email; remember it's case-insensitive (citext) |
| Subscriber emails not arriving           | Check `#/settings/smtp`; defaults to "no SMTP configured → silent no-op" |

---

## Upgrade

```bash
git pull
docker compose pull       # if using a published image
docker compose up -d --build
```

The migrator is forward-only and runs on every boot. To pin a release,
edit the `image: ghcr.io/pen-pal/rampart:latest` in `compose.yaml`
to a tagged version.

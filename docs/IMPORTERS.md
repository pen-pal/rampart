# Importers

Rampart importers are **one-shot, offline** tools for bringing existing
monitor inventories into a fresh Rampart install. They are not
background sync agents — they read a JSON / CSV / SQLite export from
disk, map each entry onto a Rampart `MonitorKind`, and INSERT through
the same repository layer the API uses. Run them once, then forget
they exist.

## Invocation

```bash
rampart-import <format> <path-to-file> [--dry-run] [--skip-existing]
```

**Required env**

| Var            | Purpose                                                |
| -------------- | ------------------------------------------------------ |
| `DATABASE_URL` | `postgres://user:pass@host:port/db` — required unless `--dry-run` |

**Flags**

| Flag              | Effect                                                                                       |
| ----------------- | -------------------------------------------------------------------------------------------- |
| `--dry-run`       | Parse + map + print summary. Do **not** insert; `DATABASE_URL` is unused in this mode.       |
| `--skip-existing` | Skip rows whose `display_name` already matches an existing monitor's `name`. Default behaviour inserts duplicates so you can re-run safely while iterating on the source export. |

**Supported `<format>` values**

| Format     | Source                                              |
| ---------- | --------------------------------------------------- |
| `site24x7`    | Site24x7 `GET /api/monitors` JSON dump (see below). |
| `pingdom`     | Pingdom `GET /api/3.1/checks` JSON dump (see below). |
| `datadog`     | Datadog `GET /api/v1/synthetics/tests` JSON dump (see below). |
| `uptimerobot` | UptimeRobot `POST /v2/getMonitors` JSON dump (see below). |

More formats are welcome — see `CONTRIBUTING.md`. The dispatch in
`crates/rampart-api/src/bin/import.rs` is a single `match` on the
first positional arg; adding a new format is "drop a sibling module
under `rampart_api::importers::` and a one-line `match` arm".

---

## Site24x7

### Getting the export

Site24x7 has no portal-side "export" button. You pull the dump
yourself via their REST API, which is OAuth 2.0 only:

```bash
# 1. Generate a self-client OAuth token in Site24x7's API console with
#    the `Site24x7.Reports.Read` + `Site24x7.Admin.Read` scopes.
# 2. Hit the monitors endpoint. Results are paginated; capture every
#    page and concat into a single {"monitors": [...]} object on disk.
curl -s 'https://www.site24x7.com/api/monitors' \
     -H 'Authorization: Zoho-oauthtoken <ACCESS_TOKEN>' \
     -H 'Accept: application/json; version=2.0' \
     > site24x7-export.json
```

> **Note:** the upstream API docs (https://www.site24x7.com/help/api/#monitors)
> describe the listing endpoint as paginated; if you have more than one
> page of monitors, fetch each page and merge their `data` arrays into
> the single `monitors` array the importer expects.

### Type → MonitorKind mapping

The importer normalises each Site24x7 `type` constant to uppercase
before lookup, so case differences across the docs don't matter.

| Site24x7 `type`                                       | Rampart `MonitorKind`                              |
| ----------------------------------------------------- | -------------------------------------------------- |
| `URL`, `HOMEPAGE`                                     | `Http` — or `Keyword` if `matching_keyword` is set |
| `RESTAPI`, `REST_API`                                 | `Http` — `Keyword` if `matching_keyword` set, `JsonQuery` if `response_content_check` set |
| `SOAP`                                                | `Http`                                             |
| `HEARTBEAT`, `CRON`                                   | `Push`                                             |
| `PING`                                                | `Ping`                                             |
| `DNS`                                                 | `Dns`                                              |
| `SSL_CERT`, `SSL_CERTIFICATE`, `SSLCERT`              | `Tls`                                              |
| `PORT`, `TCP`                                         | `Tcp`                                              |
| `POSTGRES`, `POSTGRESQL`                              | `Postgres`                                         |
| `MYSQL`                                               | `Mysql`                                            |
| `MSSQL`, `SQLSERVER`                                  | `Mssql`                                            |
| `MONGODB`, `MONGO`                                    | `Mongodb`                                          |
| `FTP`, `FTPS`, `SFTP`, `PORT_FTP`                     | `Ftp`                                              |
| `SSH`                                                 | `Ssh`                                              |
| `SMTP`, `SMTPS`, `PORT-SMTP`, `PORT_SMTP`             | `Smtp`                                             |
| `IMAP`, `IMAPS`                                       | `Imap`                                             |
| `POP`, `POP3`, `POPS`, `PORT-POP`, `PORT_POP`         | `Pop3`                                             |
| `WEBSOCKET`, `WEBSOCKETS`                             | `Websocket`                                        |
| `DOMAINEXPIRY`, `DOMAIN_EXPIRY`, `DOMAIN`             | `Domain`                                           |
| `MQTT`                                                | `Mqtt`                                             |
| `GRPC`                                                | `Grpc`                                             |

### Field translation

| Site24x7 field                          | Rampart `NewMonitor` field |
| --------------------------------------- | -------------------------- |
| `display_name`                          | `name` (required)          |
| `website` *or* `url`                    | `url`                      |
| `host_name` *or* `hostname`             | `hostname`                 |
| `port`                                  | `port` (`i32`)             |
| `check_frequency` (seconds, string/num) | `interval_seconds` (clamped to `10..=86400`; default `60`) |
| `timeout` (seconds, string/num)         | `timeout_seconds` (clamped to `1..=600`; default `16`) |
| `retries` *or* `threshold_count`        | `max_retries` (default `0`) |
| `http_method` (`G`/`P`/`U`/`D`/`H`)     | `http_method` expanded to full verb (`GET`/`POST`/…) |

Everything else on the source object is dropped, including: Site24x7
monitor groups, notification profiles, dependency settings, location
profiles, region selection, response-time thresholds, custom HTTP
headers, request body, and tags. Rampart's equivalents (where they
exist) have different semantics — port them by hand after the import.

### Types that are skipped

Anything not in the mapping table is reported as `skip:
unsupported site24x7 monitor` and ends up in the run's summary block.
Common examples:

- `REALBROWSER` — Rampart's `Browser` probe needs an external
  headless service URL (see `MonitorKind::Browser`); Site24x7's
  config doesn't carry one. Port by hand.
- `EC2INSTANCE`, `RDSINSTANCE`, `S3BUCKET`, `LAMBDAFUNCTION`,
  `EKSCLUSTER`, … — AWS introspection monitors. Out of scope per
  `CONTRIBUTING.md` (no cloud-provider scanners).
- `SERVER`, `VMWAREESX`, `VCENTER`, `BIZTALKSERVER`,
  `MSEXCHANGE`, `NETWORKDEVICE`, … — agent-based / SNMP-driven
  monitors that don't have a 1:1 Rampart probe.
- `URL-SEQ`, `FILEUPLOAD`, `MAIL-DELIVERY` — multi-step / synthetic
  flows; no equivalent.

### Example

```bash
# Dry-run first — no DB writes; just prints the per-kind breakdown +
# the list of skipped entries so you can decide whether to hand-port
# them before re-running for real.
./target/release/rampart-import site24x7 site24x7-export.json --dry-run

# Real import. Insert every mapped row.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import site24x7 site24x7-export.json

# Idempotent re-run: same source, skip rows whose name already exists.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import site24x7 site24x7-export.json \
  --skip-existing
```

The exit code is `0` on a clean run, `1` if any insert errored, and
`2` on a usage error (bad flag, missing file, etc.).

---

## Datadog

### Getting the export

Datadog has no portal-side "export" button for Synthetics. You pull
the dump yourself via their REST API, which requires an **API key**
*and* an **APP key** (both header-shaped):

```bash
# 1. In the Datadog dashboard: `Organization Settings` → `API Keys`
#    and `Application Keys`. The APP key needs the `synthetics_read`
#    scope (default for any application key) so the request is
#    permitted to enumerate Synthetics tests.
# 2. Hit the list-tests endpoint. The endpoint returns the full
#    {"tests": [...]} payload the importer expects directly — no
#    pagination glue required.
curl -s 'https://api.datadoghq.com/api/v1/synthetics/tests' \
     -H "DD-API-KEY:    $DD_API_KEY" \
     -H "DD-APPLICATION-KEY: $DD_APP_KEY" \
     > datadog-synthetics.json
```

> **Site selection:** customers on EU / US3 / US5 / AP1 sites must
> swap the host (e.g. `https://api.datadoghq.eu/...`,
> `https://api.us5.datadoghq.com/...`). The endpoint path is the
> same on every site.

### Type → MonitorKind mapping

Datadog's Synthetics surface is `(type, subtype)` shaped — `type` is
always `"api"` or `"browser"`, and `subtype` narrows API tests into
the specific protocol. The importer lowercases both fields before
lookup to be defensive against future capitalisation drift.

| Datadog `(type, subtype)`   | Rampart `MonitorKind`                                  |
| --------------------------- | ------------------------------------------------------ |
| `(api, http)`               | `Http` — or `Keyword` when `config.assertions` includes a `body` + `contains` check |
| `(api, tcp)`                | `Tcp`                                                  |
| `(api, dns)`                | `Dns`                                                  |
| `(api, ssl)`                | `Tls`                                                  |
| `(api, icmp)`               | `Ping`                                                 |
| `(api, grpc)`               | `Grpc`                                                 |
| `(api, websocket)`          | `Websocket`                                            |
| `(api, udp)`                | `Tcp` — closest connection-primitive Rampart ships; the imported probe will TCP-connect rather than UDP-send. Hand-port if the distinction matters. |
| `(browser, *)`              | `Browser` — rendered-page check. Note that Rampart's `Browser` probe needs an external headless service URL (see `MonitorKind::Browser`); the Datadog export does not carry one, so set `config.renderer_url` on the imported monitor after the run. |

### Field translation

| Datadog field                            | Rampart `NewMonitor` field |
| ---------------------------------------- | -------------------------- |
| `name`                                   | `name` (required)          |
| `config.request.url`                     | `url`                      |
| `config.request.host`                    | `hostname`                 |
| `config.request.port`                    | `port` (`i32`)             |
| `config.request.method`                  | `http_method` (uppercased; default `GET`) |
| `config.request.timeout` (seconds)       | `timeout_seconds` (clamped to `1..=600`; default `16`) |
| `options.tick_every` (seconds)           | `interval_seconds` (clamped to `10..=86400`; default `60`) |
| `options.retry.count`                    | `max_retries` (default `0`) |

Everything else on the source object is dropped, including: Datadog
locations, tags, message templates, monitor associations, alert
conditions, `config.request.body` / `headers`, response-time SLA
thresholds, and the per-step assertion list (only `body` + `contains`
is read, and only to flip HTTP into Keyword). Rampart's equivalents
(where they exist) have different semantics — port them by hand after
the import.

### Types that are skipped

Anything not in the mapping table is reported as `skip:
unsupported datadog synthetic` and ends up in the run's summary block.
The two notable cases:

- `(api, multi)` — multi-step API tests chain several requests with
  shared variables and per-step assertions. Rampart does not model
  multi-step flows; port these by hand or split them into individual
  per-endpoint probes.
- Any future / undocumented `subtype` Datadog adds — surfaced as
  `skip` so the operator sees it instead of silently coercing it to
  `Http`.

### Example

```bash
# Dry-run first — no DB writes; just prints the per-kind breakdown +
# the list of skipped entries so you can decide whether to hand-port
# them before re-running for real.
./target/release/rampart-import datadog datadog-synthetics.json --dry-run

# Real import. Insert every mapped row.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import datadog datadog-synthetics.json

# Idempotent re-run: same source, skip rows whose name already exists.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import datadog datadog-synthetics.json \
  --skip-existing
```

The exit code is `0` on a clean run, `1` if any insert errored, and
`2` on a usage error (bad flag, missing file, etc.).

---

## Pingdom

### Getting the export

Pingdom has no portal-side "export" button either — pull the dump
yourself from their REST API. Pingdom uses a long-lived bearer token
(generated under **Integrations -> The Pingdom API** in the dashboard);
the token only needs the read-only scope.

```bash
# 1. Mint an API token in the Pingdom dashboard
#    (Integrations -> The Pingdom API -> Add API token).
# 2. Hit the checks endpoint and save the raw response. The dump is the
#    same {"checks": [...]} shape the importer expects, so no merging
#    needed for installs with a single page of checks.
curl -s 'https://api.pingdom.com/api/3.1/checks' \
     -H 'Authorization: Bearer <API_TOKEN>' \
     > pingdom-export.json
```

> **Note:** the Pingdom API documents `GET /api/3.1/checks` as the
> listing endpoint (https://docs.pingdom.com/api/#tag/Checks). If your
> install has more than the default page size, pass `?limit=…` (max
> 25 000 per the docs) so a single request captures every check; merging
> multiple pages is supported — concat the `checks` arrays into one
> `{"checks": [...]}` object before running the importer.

### Type → MonitorKind mapping

The importer normalises each Pingdom `type` constant to lowercase
before lookup.

| Pingdom `type`         | Rampart `MonitorKind`                                       |
| ---------------------- | ----------------------------------------------------------- |
| `http`, `httpcustom`   | `Http` — or `Keyword` if `should_contain` is set            |
| `tcp`                  | `Tcp`                                                       |
| `udp`                  | `Tcp` (closest match — see "Caveats" below)                 |
| `dns`                  | `Dns`                                                       |
| `ping`                 | `Ping`                                                      |
| `pop3`                 | `Pop3`                                                      |
| `smtp`                 | `Smtp`                                                      |
| `imap`                 | `Imap`                                                      |
| `ssh`                  | `Ssh`                                                       |

### Field translation

| Pingdom field                              | Rampart `NewMonitor` field                                              |
| ------------------------------------------ | ----------------------------------------------------------------------- |
| `name`                                     | `name` (required)                                                       |
| `hostname` + `port` + `encryption` + `url` | `url` (HTTP shape — reconstructed as `{scheme}://{host}[:{port}]{path}`; default 80/443 dropped) |
| `hostname`                                 | `hostname` (network-primitive shape — `tcp`, `dns`, `ping`, …)          |
| `port`                                     | `port` (`i32`, network-primitive shape only)                            |
| `resolution` (minutes)                     | `interval_seconds = max(60, resolution * 60)`                           |
| `verify_certificate` (`false`)             | `ignore_tls = true`                                                     |
| `should_contain`                           | `config["keyword"]` (and flips `Http` → `Keyword`)                      |
| `should_not_contain`                       | `config["keyword_negate"]`                                              |
| `http_method`                              | `http_method` (upper-cased; e.g. `post` → `POST`)                       |

Everything else on the source object is dropped, including: Pingdom
tags, integrationids, contact lists, response time thresholds,
post-data, custom request headers, probe-location filters
(`probe_filters`), and the `ipv6` flag. Rampart's equivalents (where
they exist) have different semantics — port them by hand after the
import.

`timeout_seconds` defaults to 16s (Rampart's `NewMonitor` default) —
Pingdom's per-check timeout field isn't exposed on the listing
endpoint, so we don't try to translate it.

### Types that are skipped

Anything not in the mapping table is reported as `skip: unsupported
pingdom check` and ends up in the run's summary block. Notably:

- `transaction` — Pingdom's multi-step scripted check (login flows,
  checkout simulations). Rampart has no equivalent probe; port by
  hand or drop.

### Caveats

- **UDP**: Pingdom has a UDP probe; Rampart does not. The importer
  maps `udp` onto `Tcp` and emits a `tracing::warn!` line for each
  one so the operator can spot the gap in the log output and decide
  whether to keep the (now transport-mismatched) imported monitor.
- **`resolution` is in minutes**, not seconds. The importer multiplies
  by 60 to get Rampart's per-second `interval_seconds`. A
  `resolution` of 1 becomes a 60-second interval — Pingdom's minimum
  cadence is coarser than Rampart's so the imported monitor will
  always be at least as frequent as the original.

### Example

```bash
# Dry-run first — no DB writes; just prints the per-kind breakdown +
# the list of skipped entries so you can decide whether to hand-port
# them before re-running for real.
./target/release/rampart-import pingdom pingdom-export.json --dry-run

# Real import. Insert every mapped row.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import pingdom pingdom-export.json

# Idempotent re-run: same source, skip rows whose name already exists.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import pingdom pingdom-export.json \
  --skip-existing
```

---

## UptimeRobot

### Getting the export

UptimeRobot has no portal-side "export" button. You pull the dump
yourself via their REST API, which is API-key-only (no OAuth):

```bash
# 1. Mint a "Main API Key" in the UptimeRobot dashboard under
#    My Settings → API Settings. Read-only keys work.
# 2. Hit getMonitors. The API is form-encoded POST + JSON response;
#    `format=json` is required, and `api_key` lives in the body.
#    Results are paginated (`offset` / `limit`, default 50, max 50);
#    capture every page and merge the `monitors` arrays into a single
#    object on disk.
curl -s -X POST 'https://api.uptimerobot.com/v2/getMonitors' \
     -H 'Content-Type: application/x-www-form-urlencoded' \
     -H 'Cache-Control: no-cache' \
     -d 'api_key=<API_KEY>&format=json' \
     > uptimerobot-export.json
```

> **Note:** the response wraps the array in a `{"stat":"ok","monitors":
> [...]}` envelope; the importer reads `monitors` and ignores the rest
> of the envelope. If you have more than one page of monitors, fetch
> each page (`&offset=50`, `&offset=100`, …) and merge their `monitors`
> arrays into the single array the importer expects.

### Type → MonitorKind mapping

UptimeRobot ships `type` (and, for type 4, `sub_type`) as bare ints.
The importer reads them directly; case / casing doesn't matter.

| UptimeRobot `type` (`sub_type`) | Rampart `MonitorKind` |
| ------------------------------- | --------------------- |
| `1` (HTTP / HTTPS)              | `Http`                |
| `2` (Keyword)                   | `Keyword` (substring carried as `config["keyword"]`) |
| `3` (Ping)                      | `Ping`                |
| `4` / `1` (Port — HTTP)         | `Tcp` (default port `80` when absent)  |
| `4` / `2` (Port — HTTPS)        | `Tcp` (default port `443` when absent) |
| `4` / `3` (Port — FTP)          | `Ftp`                 |
| `4` / `4` (Port — SMTP)         | `Smtp`                |
| `4` / `5` (Port — POP3)         | `Pop3`                |
| `4` / `6` (Port — IMAP)         | `Imap`                |
| `4` / `99` (Port — Custom)      | `Tcp` (port carried explicitly) |
| `5` (Heartbeat)                 | `Push`                |

### Field translation

| UptimeRobot field                                       | Rampart `NewMonitor` field |
| ------------------------------------------------------- | -------------------------- |
| `friendly_name`                                         | `name` (required)          |
| `url` (types `1`, `2`)                                  | `url`                      |
| `url` (type `3`, host-only, parsed)                     | `hostname`                 |
| `url` (type `4`, host-only, parsed)                     | `hostname`                 |
| `port` (type `4`)                                       | `port` (`i32`)             |
| `keyword_value` (type `2`)                              | `config["keyword"]`        |
| `interval` (seconds, number/string)                     | `interval_seconds` (clamped to `10..=86400`; default `60`) |
| `timeout` (seconds, number/string)                      | `timeout_seconds` (clamped to `1..=600`; default `16`)     |

Everything else on the source object is dropped, including:
UptimeRobot monitor groups (PSPs), alert contacts, maintenance windows,
custom HTTP headers, basic-auth credentials, response-time thresholds,
SSL certificate-expiry reminders, and tags. Rampart's equivalents
(where they exist) have different semantics — port them by hand after
the import.

For port monitors (`type: 4`), the importer extracts the host from the
`url` field — UptimeRobot stores the bare host there in the v2 API, but
older exports may include a `scheme://` prefix or trailing path. We
strip both so the resulting `hostname` is a clean DNS label.

### Types that are skipped

Anything not in the mapping table is reported as `skip: unsupported
uptimerobot monitor` and ends up in the run's summary block. Common
examples:

- Unknown future `type` codes (anything other than `1` / `2` / `3` /
  `4` / `5`) — UptimeRobot occasionally introduces new monitor classes
  ahead of documenting them. Hand-port by re-creating in the Rampart UI.
- Port monitors with unknown `sub_type` codes (anything other than
  `1` / `2` / `3` / `4` / `5` / `6` / `99`) — same story; skip + manual.

### Example

```bash
# Dry-run first — no DB writes; just prints the per-kind breakdown +
# the list of skipped entries so you can decide whether to hand-port
# them before re-running for real.
./target/release/rampart-import uptimerobot uptimerobot-export.json --dry-run

# Real import. Insert every mapped row.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import uptimerobot uptimerobot-export.json

# Idempotent re-run: same source, skip rows whose name already exists.
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart \
  ./target/release/rampart-import uptimerobot uptimerobot-export.json \
  --skip-existing
```

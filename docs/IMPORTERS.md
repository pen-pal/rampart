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
| `site24x7` | Site24x7 `GET /api/monitors` JSON dump (see below). |

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

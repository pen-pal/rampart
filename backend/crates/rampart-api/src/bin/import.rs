//! `rampart-import` — one-shot CLI that ingests monitor exports from
//! other self-hosted uptime dashboards and writes them into Rampart's
//! Postgres via the existing repository helpers.
//!
//! ```text
//! rampart-import <format> <path-to-file> [--dry-run] [--skip-existing]
//! ```
//!
//! Supported formats today: `site24x7`, `pingdom`, `datadog`,
//! `uptimerobot`, `betterstack`, `healthchecks`, `cronitor`,
//! `statuscake`, `rapidspike`, `updown`, `cachet`, `gatus`,
//! `uptimecom`, `hetrixtools`, `freshping`, `checkly`, `statusgator`,
//! `pingometer`, and the Rampart-native `csv` bulk format. Adding a new
//! format means dropping a sibling module
//! under `rampart_api::importers::` and a match arm in `run()` below.
//!
//! Argument parsing is hand-rolled (no `clap`) so this bin doesn't
//! drag a new dep into the workspace — matches the existing
//! convention for the other Rampart binaries.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use rampart_api::importers::{
    betterstack, cachet, checkly, cronitor, csv_import, datadog, freshping, gatus, healthchecks,
    hetrixtools, pingdom, pingometer, rapidspike, site24x7, statuscake, statusgator, updown,
    uptimecom, uptimerobot, ImportPlan,
};
use rampart_db::monitors;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
usage: rampart-import <format> <path-to-file> [--dry-run] [--skip-existing]

formats:
  site24x7      Site24x7      `GET /api/monitors` JSON dump
  pingdom       Pingdom       `GET /api/3.1/checks` JSON dump
  datadog       Datadog       `GET /api/v1/synthetics/tests` JSON dump
  uptimerobot   UptimeRobot   `POST /v2/getMonitors` JSON dump
  betterstack   BetterStack   `GET /api/v2/monitors` JSON dump
  healthchecks  Healthchecks  `GET /api/v3/checks/` JSON dump
  cronitor      Cronitor      `GET /api/monitors` JSON dump
  statuscake    StatusCake    `GET /v1/uptime` JSON dump
  rapidspike    RapidSpike    `GET /v1/status-monitors` JSON dump
  updown        Updown.io     `GET /api/checks` JSON dump
  cachet        Cachet        `GET /api/v1/components` JSON dump
  gatus         Gatus         `GET /api/v1/endpoints/statuses` JSON dump
  uptimecom     Uptime.com    `GET /api/v1/checks/` JSON dump
  hetrixtools   HetrixTools   `GET /v1/uptime-monitors` JSON dump
  freshping     Freshping     check export JSON dump
  checkly       Checkly       `GET /v1/checks` JSON dump
  statusgator   StatusGator   services export JSON dump
  pingometer    Pingometer    monitors export JSON dump
  csv           Generic CSV   Rampart-native CSV (header row required)

flags:
  --dry-run         parse + map; print summary; do NOT insert
  --skip-existing   skip rows whose display_name already exists in the DB
                    (default behaviour inserts duplicates)

env:
  DATABASE_URL      postgres://user:pass@host:port/db   (required)
";

#[derive(Debug)]
struct Args {
    format: String,
    path: PathBuf,
    dry_run: bool,
    skip_existing: bool,
}

fn parse_args() -> Result<Args, String> {
    // argv[0] is the binary name; positional args follow.
    let mut argv = env::args().skip(1);
    let format = argv.next().ok_or_else(|| "missing <format>".to_string())?;
    if format == "-h" || format == "--help" || format == "help" {
        return Err("help".into());
    }
    let path = argv
        .next()
        .ok_or_else(|| "missing <path-to-file>".to_string())?;

    let mut dry_run = false;
    let mut skip_existing = false;
    for arg in argv {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--skip-existing" => skip_existing = true,
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(Args {
        format,
        path: PathBuf::from(path),
        dry_run,
        skip_existing,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            if msg == "help" {
                eprintln!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {msg}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match run(args).await {
        Ok(code) => code,
        Err(e) => {
            // Print to stderr for the operator + log it so structured-
            // log shipper picks it up too if they wrapped the cmd.
            error!(error = %e, "import failed");
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> anyhow::Result<ExitCode> {
    let raw = std::fs::read_to_string(&args.path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", args.path.display()))?;

    let plan: ImportPlan = match args.format.as_str() {
        "site24x7" => site24x7::parse_and_map(&raw)?,
        "pingdom" => pingdom::parse_and_map(&raw)?,
        "datadog" => datadog::parse_and_map(&raw)?,
        "uptimerobot" => uptimerobot::parse_and_map(&raw)?,
        "betterstack" => betterstack::parse_and_map(&raw)?,
        "healthchecks" => healthchecks::parse_and_map(&raw)?,
        "cronitor" => cronitor::parse_and_map(&raw)?,
        "statuscake" => statuscake::parse_and_map(&raw)?,
        "rapidspike" => rapidspike::parse_and_map(&raw)?,
        "updown" => updown::parse_and_map(&raw)?,
        "cachet" => cachet::parse_and_map(&raw)?,
        "gatus" => gatus::parse_and_map(&raw)?,
        "uptimecom" => uptimecom::parse_and_map(&raw)?,
        "hetrixtools" => hetrixtools::parse_and_map(&raw)?,
        "freshping" => freshping::parse_and_map(&raw)?,
        "checkly" => checkly::parse_and_map(&raw)?,
        "statusgator" => statusgator::parse_and_map(&raw)?,
        "pingometer" => pingometer::parse_and_map(&raw)?,
        // CSV is not JSON: it has its own entry point that parses the
        // raw file text as a Rampart-native CSV table rather than a
        // vendor JSON export.
        "csv" => csv_import::parse_csv_and_map(&raw)?,
        other => {
            anyhow::bail!(
                "unsupported format `{other}` — supported: site24x7, pingdom, datadog, uptimerobot, betterstack, healthchecks, cronitor, statuscake, rapidspike, updown, cachet, gatus, uptimecom, hetrixtools, freshping, checkly, statusgator, pingometer, csv. {USAGE}",
                USAGE = "Use --help for the full list."
            );
        }
    };

    info!(
        format = %args.format,
        mapped = plan.mapped.len(),
        skipped = plan.skipped.len(),
        "parsed export",
    );

    // Print a per-kind breakdown so the operator can sanity-check the
    // mapping before letting it loose on prod.
    print_breakdown(&plan);

    if args.dry_run {
        info!("--dry-run set; not inserting");
        println!(
            "dry-run: would import {} monitors; skipped {} unsupported entries.",
            plan.mapped.len(),
            plan.skipped.len(),
        );
        return Ok(ExitCode::SUCCESS);
    }

    // Real run — needs a DB pool.
    let database_url =
        env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL not set"))?;
    let pool = rampart_db::connect(&database_url, 4).await?;

    // Build the existing-name set up front (single SELECT) when the
    // operator wants dedup; cheaper than a per-row lookup and matches
    // how the API's own dashboard renders the list.
    let existing: std::collections::HashSet<String> = if args.skip_existing {
        monitors::list_all(&pool)
            .await?
            .into_iter()
            .map(|m| m.name)
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let mut imported = 0usize;
    let mut skipped_dupe = 0usize;
    let mut errors = 0usize;
    for m in plan.mapped {
        if args.skip_existing && existing.contains(&m.new_monitor.name) {
            warn!(name = %m.new_monitor.name, "skip: monitor with this name already exists");
            skipped_dupe += 1;
            continue;
        }
        // P5: resolve org from ingest credential — the CLI importer has no
        // request context, so everything lands in the Default org.
        match monitors::create(
            &pool,
            m.new_monitor,
            rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID),
        )
        .await
        {
            Ok(monitor) => {
                info!(name = %monitor.name, kind = ?monitor.kind, id = %monitor.id.0, "imported");
                imported += 1;
            }
            Err(e) => {
                error!(name = %m.source_name, error = %e, "insert failed");
                errors += 1;
            }
        }
    }

    println!(
        "imported {} monitors; skipped {} duplicates; skipped {} unsupported entries; {} errors.",
        imported,
        skipped_dupe,
        plan.skipped.len(),
        errors,
    );
    if errors > 0 {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

/// Group `plan.mapped` by `MonitorKind` and print a one-line summary
/// per kind. Operators tend to know how many of each probe type they
/// expect — divergence flags a mapping bug fast.
fn print_breakdown(plan: &ImportPlan) {
    use std::collections::BTreeMap;
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for m in &plan.mapped {
        let key = format!("{:?}", m.mapped_kind);
        *by_kind.entry(key).or_insert(0) += 1;
    }
    println!("mapping summary:");
    if by_kind.is_empty() {
        println!("  (none)");
    } else {
        for (kind, n) in &by_kind {
            println!("  {kind:<10} {n}");
        }
    }
    if !plan.skipped.is_empty() {
        println!("skipped (no Rampart equivalent):");
        for s in &plan.skipped {
            println!("  {:<18} {}  ({})", s.source_kind, s.source_name, s.reason);
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

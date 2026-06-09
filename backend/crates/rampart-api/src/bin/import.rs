//! `rampart-import` — one-shot CLI that ingests monitor exports from
//! other self-hosted uptime dashboards and writes them into Rampart's
//! Postgres via the existing repository helpers.
//!
//! ```text
//! rampart-import <format> <path-to-file> [--dry-run] [--skip-existing]
//! ```
//!
//! Supported formats today: `site24x7`, `pingdom`, `datadog`,
//! `uptimerobot`, `betterstack`, `healthchecks`, `cronitor`. Adding a
//! new format means dropping a sibling module under
//! `rampart_api::importers::` and a match arm in `run()` below.
//!
//! Argument parsing is hand-rolled (no `clap`) so this bin doesn't
//! drag a new dep into the workspace — matches the existing
//! convention for the other Rampart binaries.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use rampart_api::importers::{
    betterstack, cronitor, datadog, healthchecks, pingdom, site24x7, uptimerobot, ImportPlan,
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
        other => {
            anyhow::bail!(
                "unsupported format `{other}` — supported: site24x7, pingdom, datadog, uptimerobot, betterstack, healthchecks, cronitor. {USAGE}",
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
        monitors::list(&pool)
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
        match monitors::create(&pool, m.new_monitor).await {
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

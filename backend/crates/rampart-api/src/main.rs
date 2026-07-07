//! Rampart API server.
//!
//! Wires:
//!   - structured tracing (JSON in prod, pretty in dev)
//!   - Postgres pool + migrations on boot
//!   - axum router (built in lib.rs) with CORS, request-id, gzip, timeouts
//!   - graceful shutdown on SIGTERM
//!
//! Env vars:
//!   DATABASE_URL          (required) postgres://...
//!   BIND_ADDR             default 0.0.0.0:3000
//!   RUST_LOG              default "rampart=info,tower_http=info,info"
//!   DATABASE_POOL_SIZE    default 16
//!   RAMPART_SSRF_BLOCK_PRIVATE  "1"/"true" → probes also refuse private/
//!                         internal IP ranges (off by default; metadata +
//!                         loopback + link-local are always blocked).
//!   RAMPART_REQUIRE_INGEST_AUTH "1"/"true" → OTLP/RUM ingest is refused
//!                         unless a telemetry token is configured + presented.
//!   RAMPART_TRUSTED_PROXIES comma-separated IPs/CIDRs of the reverse proxy /
//!                         load-balancer(s) that front Rampart. Default unset =
//!                         ignore X-Forwarded-For and use the TCP peer IP for
//!                         rate-limiting + audit (secure). Set it to the
//!                         SPECIFIC proxy IP(s) (e.g. 203.0.113.10 or a /32) so
//!                         the real client is read from XFF behind that proxy.
//!                         NEVER set it to a broad range (e.g. 10.0.0.0/8) that
//!                         also holds untrusted hosts — any host inside a
//!                         trusted CIDR can forge the client IP via XFF.
//!   RAMPART_SECRET_KEY    32-byte key (64 hex / base64). When set, notification
//!                         channel secrets are AES-256-GCM encrypted at rest.
//!                         Unset → secrets stored PLAINTEXT (loud startup warn +
//!                         /healthz `secrets_at_rest:"plaintext"` + UI banner).
//!   RAMPART_REQUIRE_SECRET_KEY "1"/"true" → refuse to start unless
//!                         RAMPART_SECRET_KEY is set (fail-closed at-rest crypto).
//!   RAMPART_OIDC_ISSUER / _CLIENT_ID / _CLIENT_SECRET / _REDIRECT_URL
//!                         when all set, enables OIDC SSO login. Optional
//!                         RAMPART_OIDC_DEFAULT_ROLE = admin|editor|readonly.
//!   RAMPART_OTLP_ENDPOINT self-observability: when set (base URL, e.g.
//!                         http://localhost:3000 or a collector), Rampart
//!                         exports its OWN traces + logs there via OTLP/HTTP.
//!                         Ingest + scrape routes are excluded to avoid a
//!                         feedback loop when pointed at itself.
//!   RAMPART_SELF_RUM      "1"/"true" → inject the RUM snippet into the
//!                         dashboard shell so the UI reports its own Core Web
//!                         Vitals + browser JS errors (real RUM + error data).

use rampart_api::{build_router, state::AppState, static_assets};
use std::net::SocketAddr;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry_guards = init_tracing();

    // Install the ring `CryptoProvider` as the global default before
    // any rustls-backed client is constructed. `reqwest 0.13` is
    // configured with the `rustls-no-provider` feature so this is
    // the single decision point for "which crypto stack does the
    // whole workspace use" — it propagates to `tokio-rustls`,
    // `hyper-rustls`, and every other indirect consumer. Returns
    // `Err(())` if a provider was already installed (e.g. from a
    // test re-running this fn); we tolerate that.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL not set"))?;
    let pool_size: u32 = std::env::var("DATABASE_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16);
    let bind: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()?;

    info!(%bind, pool_size, "starting rampart-api");

    // Client-IP trust posture. With RAMPART_TRUSTED_PROXIES unset we use the
    // raw TCP peer for rate-limiting + audit and IGNORE X-Forwarded-For (the
    // secure default — XFF is client-spoofable). That's correct for a direct
    // bind, but if a reverse proxy fronts Rampart on a non-loopback address
    // the peer is the proxy, so every client collapses to one bucket / source
    // IP. Make that loud, mirroring the weak-secret startup warn.
    let trusted_proxies_set = std::env::var("RAMPART_TRUSTED_PROXIES")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    if !trusted_proxies_set && !bind.ip().is_loopback() {
        warn!(
            "RAMPART_TRUSTED_PROXIES unset — X-Forwarded-For is ignored and the TCP peer IP is \
             used for rate-limiting + audit. If a reverse proxy fronts Rampart, set \
             RAMPART_TRUSTED_PROXIES to its IP(s) or per-client rate-limits + audit source IPs \
             collapse to the proxy IP."
        );
    }

    // Multi-DB backend select (P1). The DATABASE_URL scheme picks the store:
    // `postgres://…` → PgStore (+ a concrete pool for the Postgres-only paths);
    // `sqlite:…` → SqliteStore (single-binary/homelab tier; requires this binary
    // to be built with the `sqlite` feature). The scheduler / notifier / SIEM
    // loops are backend-agnostic (they take `Arc<dyn Store>`); only the residual
    // telemetry-ingest / prune / self-metrics paths still need the raw pool.
    // `mysql://…` → MysqlStore (relational-subset tier; requires the `mysql`
    // feature). Same backend-agnostic story as sqlite: no Postgres pool, so the
    // prune / self-metrics / seed-demo paths (Postgres-only) are skipped.
    let is_sqlite = database_url.starts_with("sqlite:");
    let is_mysql = database_url.starts_with("mysql:");
    let (store, pg_pool): (
        std::sync::Arc<dyn rampart_db::store::Store>,
        Option<rampart_db::DbPool>,
    ) = if is_sqlite {
        #[cfg(feature = "sqlite")]
        {
            let s = rampart_db::sqlite::store::SqliteStore::connect(&database_url).await?;
            info!("sqlite backend: migrations applied");
            (std::sync::Arc::new(s), None)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            anyhow::bail!(
                "DATABASE_URL is a sqlite URL but this binary was built without the `sqlite` \
                 feature. Rebuild with `--features sqlite`, or use a postgres:// DATABASE_URL."
            );
        }
    } else if is_mysql {
        #[cfg(feature = "mysql")]
        {
            let s = rampart_db::mysql::store::MysqlStore::connect(&database_url).await?;
            info!("mysql backend: migrations applied");
            (std::sync::Arc::new(s), None)
        }
        #[cfg(not(feature = "mysql"))]
        {
            anyhow::bail!(
                "DATABASE_URL is a mysql URL but this binary was built without the `mysql` \
                 feature. Rebuild with `--features mysql`, or use a postgres:// DATABASE_URL."
            );
        }
    } else {
        let pool = rampart_db::connect(&database_url, pool_size).await?;
        rampart_db::migrate(&pool).await?;
        info!("migrations applied");
        (
            std::sync::Arc::new(rampart_db::store::PgStore::new(pool.clone())),
            Some(pool),
        )
    };

    // Subcommand: `rampart-api seed-demo` populates a representative dataset
    // across every tier and exits (no server) — for demos + first-run.
    if std::env::args().nth(1).as_deref() == Some("seed-demo") {
        let Some(pool) = pg_pool.as_ref() else {
            anyhow::bail!("seed-demo currently requires the postgres backend");
        };
        let stats = rampart_api::seed::run(pool).await?;
        info!(%stats, "seed-demo complete");
        println!("{stats}");
        return Ok(());
    }

    // Subcommand: `rampart-api reset-password <email> <password>` — break-glass
    // admin recovery. Resets the password if the user exists, else creates an
    // admin with that email. Bypasses the API password policy (operator's
    // choice); enforces only a minimum length. Exits without starting the
    // server.
    if std::env::args().nth(1).as_deref() == Some("reset-password") {
        let Some(pool) = pg_pool.as_ref() else {
            anyhow::bail!("reset-password currently requires the postgres backend");
        };
        let email = std::env::args().nth(2);
        let password = std::env::args().nth(3);
        let (Some(email), Some(password)) = (email, password) else {
            anyhow::bail!("usage: rampart-api reset-password <email> <password>");
        };
        if password.chars().count() < 8 {
            anyhow::bail!("password must be at least 8 characters");
        }
        let hash = rampart_api::auth::hash_password(&password)
            .map_err(|e| anyhow::anyhow!("hash failed: {e:?}"))?;
        match rampart_db::users::get_by_email(pool, &email).await {
            Ok(u) => {
                rampart_db::users::set_password(pool, u.id, &hash).await?;
                println!("password reset for existing user {email}");
            }
            Err(rampart_db::DbError::NotFound) => {
                rampart_db::users::create(
                    pool,
                    rampart_db::users::NewUser {
                        email: email.clone(),
                        name: None,
                        password_hash: hash,
                        role: rampart_core::Role::Admin,
                    },
                )
                .await?;
                println!("created admin user {email}");
            }
            Err(e) => return Err(e.into()),
        }
        // Self-check: re-read the row and verify the hash in-process, so a green
        // line proves the credential is good in THIS database — isolating a
        // login failure to a typo or a different server/DB instance.
        match rampart_db::users::get_by_email(pool, &email).await {
            Ok(u) if rampart_api::auth::verify_password(&password, &u.password_hash) => {
                println!("verified: '{email}' logs in with this password against {database_url}");
            }
            Ok(_) => println!("WARNING: stored hash did NOT verify — login would fail"),
            Err(e) => println!("WARNING: could not re-read user for verify: {e}"),
        }
        return Ok(());
    }

    // Secrets-at-rest posture. Notification-channel credentials (webhook
    // tokens, SMTP passwords, the API keys of 128 channels) live in JSONB;
    // without RAMPART_SECRET_KEY they are stored as PLAINTEXT. Make that loud
    // rather than silent, and let operators enforce encryption fail-closed the
    // same way RAMPART_REQUIRE_INGEST_AUTH gates ingest.
    if rampart_db::secrets::weak_key_configured() {
        anyhow::bail!(
            "RAMPART_SECRET_KEY is set but is dangerously low-entropy (looks like a placeholder \
             — e.g. all-zeros or a repeated byte). Every channel secret would be encrypted under \
             a guessable key, which is worse than plaintext (false assurance). Provide a real \
             32-byte random key, e.g. `openssl rand -hex 32`."
        );
    }
    if rampart_db::secrets::is_enabled() {
        info!(
            "secrets-at-rest: channel credentials encrypted (AES-256-GCM via RAMPART_SECRET_KEY)"
        );
    } else if require_secret_key() {
        anyhow::bail!(
            "RAMPART_REQUIRE_SECRET_KEY is set but RAMPART_SECRET_KEY is missing or invalid — \
             refusing to start with plaintext channel secrets. Provide a 32-byte key \
             (64 hex chars or base64)."
        );
    } else {
        warn!(
            "SECURITY: RAMPART_SECRET_KEY is not set — notification-channel credentials \
             (webhook tokens, SMTP passwords, API keys) are stored as PLAINTEXT in the database. \
             Set RAMPART_SECRET_KEY (32 bytes: 64 hex chars or base64) to enable AES-256-GCM \
             encryption at rest, or RAMPART_REQUIRE_SECRET_KEY=1 to refuse startup without it."
        );
    }

    // Symmetric to the secrets-at-rest warning: if the public telemetry-ingest
    // surface is reachable with no auth configured, anonymous clients can write
    // telemetry into the Default org. Flag it loudly so an operator who exposed
    // the port notices (matches the RAMPART_REQUIRE_INGEST_AUTH fail-closed knob).
    if !rampart_api::ingest_util::multi_org_enabled()
        && !rampart_api::ingest_util::require_ingest_auth()
    {
        let token_set = store
            .get_setting(rampart_api::ingest_util::TELEMETRY_TOKEN_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(|s| !s.trim().is_empty()))
            .unwrap_or(false);
        if !token_set {
            warn!(
                "SECURITY: the telemetry-ingest endpoints (OTLP / Prometheus / RUM / syslog / \
                 profiles) accept UNAUTHENTICATED writes into the Default org — no \
                 RAMPART_MULTI_ORG, no RAMPART_REQUIRE_INGEST_AUTH, and no `telemetry_token` set. \
                 Fine on a private/trusted network; if the ingest port is publicly reachable set \
                 RAMPART_REQUIRE_INGEST_AUTH=1 (or mint ingest keys + RAMPART_MULTI_ORG=1) to \
                 require a credential."
            );
        }
    }

    // Leader election. Only the replica holding the Postgres advisory lock
    // runs the scheduler / notifier digest flush / retention prune, so a
    // multi-replica deployment never double-probes or double-pages. On a
    // single replica the lock is acquired immediately (no behaviour change).
    // The HTTP API below runs on every replica regardless. The advisory-lock
    // election is Postgres-only; the single-binary SQLite tier is always leader.
    // Non-Postgres tiers (SQLite, MySQL) have no PG pool / advisory lock → always leader.
    let leadership = if pg_pool.is_none() {
        rampart_db::leader::Leadership::always()
    } else {
        rampart_db::leader::spawn(database_url.clone())
    };

    // Bring up the notifier service first so the scheduler can hand
    // events to it as soon as a monitor flips status.
    let (notifier_service, notifier_handle) = rampart_notifier::NotifierService::new(store.clone());
    let notifier_leadership = leadership.clone();
    tokio::spawn(async move {
        notifier_service.run(notifier_leadership).await;
    });
    info!("notifier service started");

    // Bring up the scheduler. It owns its own probe tasks and writer
    // task; we just hand it the DB pool + a notifier handle so flips
    // emit events. The reload handle lets API routes poke it after
    // monitor mutations.
    let scheduler = std::sync::Arc::new(rampart_scheduler::Scheduler::with_notifier(
        store.clone(),
        Some(notifier_handle.clone()),
    ));
    let reload_handle = scheduler.reload_handle();
    let scheduler_for_run = scheduler.clone();
    let scheduler_leadership = leadership.clone();
    tokio::spawn(async move {
        scheduler_for_run.run(scheduler_leadership).await;
    });
    info!("scheduler started");

    // Background retention prune — hourly, leader-gated, best-effort. Runs
    // through the `Store` seam so EVERY backend prunes: PgStore runs the full
    // rollup-tiered sweep; MySQL/SQLite run a flat age-based prune of the same
    // telemetry tables (no rollup tier yet). Reads thresholds from
    // settings.retention_days; failures log but don't kill the task.
    let prune_store = store.clone();
    let prune_leadership = leadership.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
        ticker.tick().await; // skip the immediate tick — let the scheduler warm up
        loop {
            ticker.tick().await;
            // Leader-only: one prune pass across the cluster, no racing DELETEs.
            if !prune_leadership.is_leader() {
                continue;
            }
            match prune_store.run_retention_prune().await {
                Ok(0) => {}
                Ok(rows) => info!(rows, "retention prune complete"),
                Err(e) => warn!(error = %e, "retention prune failed"),
            }
        }
    });
    info!("retention prune loop started");

    // SIEM / syslog export — leader-gated forward tail of the audit log to an
    // external sink (configured in settings; disabled by default).
    // The SIEM loop runs entirely through the object-safe `Store` seam.
    let siem_store = store.clone();
    let siem_leadership = leadership.clone();
    tokio::spawn(async move {
        rampart_notifier::siem::run_loop(
            siem_store,
            siem_leadership,
            std::time::Duration::from_secs(15),
        )
        .await;
    });
    info!("siem export loop started");

    static_assets::log_state();
    let state = match &pg_pool {
        Some(pool) => AppState::with_scheduler(pool.clone(), reload_handle, scheduler.clone()),
        None => AppState::with_scheduler_store(store.clone(), reload_handle, scheduler.clone()),
    };
    // Self-metrics: snapshot our own HTTP counters into the metric tier every
    // minute so the in-app Metrics view shows Rampart's live request rate +
    // latency. Postgres-only for now (writes via the metric free-fns on a pool).
    if let Some(pool) = pg_pool.as_ref() {
        tokio::spawn(rampart_api::self_metrics::run(
            state.http_metrics().clone(),
            pool.clone(),
        ));
    }
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    // `into_make_service_with_connect_info::<SocketAddr>` so the outermost
    // client-IP middleware can read the real TCP peer (ConnectInfo) for
    // trusted rate-limiting + audit, rather than a spoofable X-Forwarded-For.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    // Flush the self-telemetry batch exporters before exit.
    if let Some(g) = telemetry_guards {
        g.shutdown();
    }

    info!("rampart-api stopped cleanly");
    Ok(())
}

/// Whether encryption-at-rest is mandatory (`RAMPART_REQUIRE_SECRET_KEY`
/// = `1`/`true`/`yes`). Mirrors `RAMPART_REQUIRE_INGEST_AUTH`: when set, the
/// process refuses to start unless a valid `RAMPART_SECRET_KEY` is configured.
fn require_secret_key() -> bool {
    matches!(
        std::env::var("RAMPART_REQUIRE_SECRET_KEY").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

fn init_tracing() -> Option<rampart_api::self_telemetry::Guards> {
    use tracing_subscriber::prelude::*;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("rampart=info,tower_http=info,info"));

    // Production deploys typically ship logs to a structured aggregator
    // (Loki / Datadog / Splunk / SaaS log providers). `RAMPART_LOG_FORMAT
    // =json` swaps the human-readable formatter for the JSON one so the
    // aggregator can index `request_id` as a first-class field rather
    // than re-parsing it out of the log text. Default stays compact +
    // human-readable for dev / `docker compose up` use.
    let json = std::env::var("RAMPART_LOG_FORMAT").as_deref() == Ok("json");
    let fmt_layer = if json {
        tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .flatten_event(true)
            .with_target(true)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer().with_target(true).boxed()
    };

    // Self-observability: when RAMPART_OTLP_ENDPOINT is set, also export our own
    // traces + logs there (the example points it at Rampart itself).
    let (otel_layers, guards) = match std::env::var("RAMPART_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty())
    {
        Some(ep) => match rampart_api::self_telemetry::build(&ep) {
            Ok((layers, g)) => (layers, Some(g)),
            Err(e) => {
                eprintln!("self-telemetry disabled: {e}");
                (Vec::new(), None)
            }
        },
        None => (Vec::new(), None),
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layers)
        .init();
    guards
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            warn!(error = %e, "failed to install ctrl-c handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => warn!(error = %e, "failed to install SIGTERM handler"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("received SIGINT, shutting down"),
        _ = terminate => info!("received SIGTERM, shutting down"),
    }
}

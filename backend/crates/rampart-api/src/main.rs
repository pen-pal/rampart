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

    let pool = rampart_db::connect(&database_url, pool_size).await?;
    rampart_db::migrate(&pool).await?;
    info!("migrations applied");

    // Subcommand: `rampart-api seed-demo` populates a representative dataset
    // across every tier and exits (no server) — for demos + first-run.
    if std::env::args().nth(1).as_deref() == Some("seed-demo") {
        let stats = rampart_api::seed::run(&pool).await?;
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
        match rampart_db::users::get_by_email(&pool, &email).await {
            Ok(u) => {
                rampart_db::users::set_password(&pool, u.id, &hash).await?;
                println!("password reset for existing user {email}");
            }
            Err(rampart_db::DbError::NotFound) => {
                rampart_db::users::create(
                    &pool,
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
        match rampart_db::users::get_by_email(&pool, &email).await {
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

    // Leader election. Only the replica holding the Postgres advisory lock
    // runs the scheduler / notifier digest flush / retention prune, so a
    // multi-replica deployment never double-probes or double-pages. On a
    // single replica the lock is acquired immediately (no behaviour change).
    // The HTTP API below runs on every replica regardless.
    let leadership = rampart_db::leader::spawn(database_url.clone());

    // Bring up the notifier service first so the scheduler can hand
    // events to it as soon as a monitor flips status.
    let (notifier_service, notifier_handle) = rampart_notifier::NotifierService::new(pool.clone());
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
        pool.clone(),
        Some(notifier_handle.clone()),
    ));
    let reload_handle = scheduler.reload_handle();
    let scheduler_for_run = scheduler.clone();
    let scheduler_leadership = leadership.clone();
    tokio::spawn(async move {
        scheduler_for_run.run(scheduler_leadership).await;
    });
    info!("scheduler started");

    // Background retention prune — hourly DELETEs against heartbeats +
    // audit_log. Reads thresholds from settings.retention_days (90 / 365
    // days by default). Best-effort; failures log but don't kill the
    // task.
    let prune_pool = pool.clone();
    let prune_leadership = leadership.clone();
    tokio::spawn(async move {
        rampart_db::prune::run_loop(
            prune_pool,
            std::time::Duration::from_secs(3600),
            prune_leadership,
        )
        .await;
    });
    info!("retention prune loop started");

    // SIEM / syslog export — leader-gated forward tail of the audit log to an
    // external sink (configured in settings; disabled by default).
    // The SIEM loop runs entirely through the object-safe `Store` seam (multi-DB
    // P1 seam-plumbing), so it works against any backend; build the store from
    // the same pool (no I/O).
    let siem_store: std::sync::Arc<dyn rampart_db::store::Store> =
        std::sync::Arc::new(rampart_db::store::PgStore::new(pool.clone()));
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
    let state = AppState::with_scheduler(pool, reload_handle, scheduler.clone());
    // Self-metrics: snapshot our own HTTP counters into the metric tier every
    // minute so the in-app Metrics view shows Rampart's live request rate +
    // latency (not just externally-pushed series).
    tokio::spawn(rampart_api::self_metrics::run(
        state.http_metrics().clone(),
        state.pool().clone(),
    ));
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

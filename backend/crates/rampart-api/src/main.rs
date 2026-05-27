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

use rampart_api::{build_router, state::AppState, static_assets};
use std::net::SocketAddr;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL not set"))?;
    let pool_size: u32 = std::env::var("DATABASE_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16);
    let bind: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()?;

    info!(%bind, pool_size, "starting rampart-api");

    let pool = rampart_db::connect(&database_url, pool_size).await?;
    rampart_db::migrate(&pool).await?;
    info!("migrations applied");

    // Bring up the notifier service first so the scheduler can hand
    // events to it as soon as a monitor flips status.
    let (notifier_service, notifier_handle) = rampart_notifier::NotifierService::new(pool.clone());
    tokio::spawn(async move { notifier_service.run().await; });
    info!("notifier service started");

    // Bring up the scheduler. It owns its own probe tasks and writer
    // task; we just hand it the DB pool + a notifier handle so flips
    // emit events. The reload handle lets API routes poke it after
    // monitor mutations.
    let scheduler = std::sync::Arc::new(
        rampart_scheduler::Scheduler::with_notifier(pool.clone(), Some(notifier_handle.clone()))
    );
    let reload_handle = scheduler.reload_handle();
    let scheduler_for_run = scheduler.clone();
    tokio::spawn(async move { scheduler_for_run.run().await; });
    info!("scheduler started");

    static_assets::log_state();
    let state = AppState::new(pool, reload_handle);
    let app   = build_router(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("rampart-api stopped cleanly");
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("rampart=info,tower_http=info,info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
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
            Ok(mut s) => { s.recv().await; }
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

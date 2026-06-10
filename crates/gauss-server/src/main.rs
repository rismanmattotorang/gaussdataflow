use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use gauss_server::{registry, AppState};
use gauss_store::Store;

#[derive(Parser)]
#[command(name = "gauss-server", version, about = "gaussdataflow config API")]
struct Cli {
    /// Postgres connection string
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
    /// Address to listen on
    #[arg(long, env = "GAUSS_BIND", default_value = "127.0.0.1:8000")]
    bind: SocketAddr,
    /// Optional connector-registry JSON to import at startup (idempotent)
    #[arg(long)]
    seed_registry: Option<PathBuf>,
    /// Run the orchestration worker (job queue + scheduler) in-process
    #[arg(long, env = "GAUSS_WORKER")]
    worker: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let cli = Cli::parse();

    let store = Store::connect(&cli.database_url)
        .await
        .context("connecting to database (migrations run automatically)")?;
    tracing::info!("database connected, migrations applied");

    if let Some(path) = &cli.seed_registry {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading registry seed {}", path.display()))?;
        let doc = serde_json::from_str(&raw).context("parsing registry seed")?;
        let summary = registry::import(&store, doc).await?;
        tracing::info!(
            sources = summary.sources,
            destinations = summary.destinations,
            "registry seeded"
        );
    }

    let state = AppState::new(store.clone());

    if cli.worker {
        let orchestrator = std::sync::Arc::new(gauss_orchestrator::Orchestrator::new(
            store,
            state.secrets.clone(),
            gauss_orchestrator::WorkerOptions::default(),
        ));
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        // Leak the sender: the worker runs for the process lifetime.
        std::mem::forget(_shutdown_tx);
        tokio::spawn(orchestrator.run(shutdown_rx));
        tracing::info!("orchestration worker enabled");
    }

    let app = gauss_server::app(state);
    let listener = tokio::net::TcpListener::bind(cli.bind).await?;
    tracing::info!("gauss-server listening on http://{}", cli.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

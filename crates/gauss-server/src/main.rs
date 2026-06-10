use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use gauss_secrets::SecretsBackend;
use gauss_server::{auth, import, registry, AppState};
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
    /// Require a valid API token on every /api/v1 request
    #[arg(long, env = "GAUSS_REQUIRE_AUTH")]
    require_auth: bool,
    /// Secrets backend: postgres (default) or vault
    #[arg(long, env = "GAUSS_SECRETS_BACKEND", default_value = "postgres")]
    secrets_backend: String,
    /// Create an API token (`<name>:<role>`), print it, and exit
    #[arg(long, value_name = "NAME:ROLE")]
    create_token: Option<String>,
    /// Import a deployment document (workspace + actors + connections), then exit
    #[arg(long)]
    import_file: Option<PathBuf>,
}

fn vault_backend() -> anyhow::Result<gauss_secrets::VaultSecretsBackend> {
    let addr = std::env::var("VAULT_ADDR").context("vault backend needs VAULT_ADDR")?;
    let token = std::env::var("VAULT_TOKEN").context("vault backend needs VAULT_TOKEN")?;
    let mount = std::env::var("VAULT_MOUNT").unwrap_or_else(|_| "secret".into());
    let prefix = std::env::var("VAULT_PREFIX").unwrap_or_else(|_| "gaussdataflow".into());
    Ok(gauss_secrets::VaultSecretsBackend::new(
        addr, token, mount, prefix,
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        // stdout is reserved for command output (--create-token prints the
        // raw token there); logs go to stderr.
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let store = Store::connect(&cli.database_url)
        .await
        .context("connecting to database (migrations run automatically)")?;
    tracing::info!("database connected, migrations applied");

    let secrets: Arc<dyn SecretsBackend> = match cli.secrets_backend.as_str() {
        "postgres" => Arc::new(store.secrets_backend()),
        "vault" => Arc::new(vault_backend()?),
        other => anyhow::bail!("unknown secrets backend `{other}` (postgres|vault)"),
    };

    // One-shot administrative commands.
    if let Some(spec) = &cli.create_token {
        let (name, role) = spec
            .split_once(':')
            .context("--create-token expects <name>:<role>")?;
        anyhow::ensure!(
            auth::Role::parse(role).is_some(),
            "role must be admin, editor, or viewer"
        );
        let raw = auth::generate_token();
        store
            .tokens()
            .create(name, role, &auth::hash_token(&raw))
            .await?;
        println!("{raw}");
        eprintln!("token `{name}` ({role}) created — the value above is shown once");
        return Ok(());
    }
    if let Some(path) = &cli.import_file {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let doc = serde_json::from_str(&raw).context("parsing import document")?;
        let summary = import::import(&store, secrets.as_ref(), doc).await?;
        println!(
            "imported: {} source(s), {} destination(s), {} connection(s)",
            summary.sources, summary.destinations, summary.connections
        );
        return Ok(());
    }

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

    let state = AppState::with_secrets(store.clone(), secrets).require_auth(cli.require_auth);
    if cli.require_auth {
        tracing::info!("API token authentication required");
    }

    if cli.worker {
        let orchestrator = Arc::new(gauss_orchestrator::Orchestrator::new(
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

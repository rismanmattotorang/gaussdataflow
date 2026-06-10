//! `gauss` — the gaussdataflow connector dev loop.
//!
//! Drives any Airbyte-protocol connector, either a Docker image (`--image
//! airbyte/source-faker:latest`) or a local binary (`--exec ./connector`):
//!
//! ```text
//! gauss spec     --image airbyte/source-faker:latest
//! gauss check    --image … --config config.json
//! gauss discover --image … --config config.json
//! gauss read     --image … --config config.json [--catalog catalog.json | --full-refresh] [--state state.json]
//! ```

use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use gauss_connector_runtime::{ConnectorRunner, DockerLauncher, ProcessLauncher, ReadEvent};
use gauss_protocol::{ConfiguredAirbyteCatalog, ConfiguredAirbyteStream};

#[derive(Parser)]
#[command(
    name = "gauss",
    version,
    about = "gaussdataflow connector dev loop (Airbyte-protocol compatible)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct ConnectorArgs {
    /// Docker image of the connector (e.g. airbyte/source-faker:latest)
    #[arg(long, conflicts_with = "exec", global = false)]
    image: Option<String>,
    /// Path to a local connector binary instead of a Docker image
    #[arg(long)]
    exec: Option<PathBuf>,
}

impl ConnectorArgs {
    fn runner(&self) -> Result<ConnectorRunner> {
        match (&self.image, &self.exec) {
            (Some(image), None) => Ok(ConnectorRunner::new(DockerLauncher::new(image))),
            (None, Some(program)) => Ok(ConnectorRunner::new(ProcessLauncher::new(program))),
            _ => bail!("exactly one of --image or --exec is required"),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Print the connector's configuration specification
    Spec {
        #[command(flatten)]
        connector: ConnectorArgs,
    },
    /// Validate a configuration against the connector
    Check {
        #[command(flatten)]
        connector: ConnectorArgs,
        #[arg(long)]
        config: PathBuf,
    },
    /// List the streams the connector can produce
    Discover {
        #[command(flatten)]
        connector: ConnectorArgs,
        #[arg(long)]
        config: PathBuf,
    },
    /// Read records (NDJSON on stdout, summary on stderr)
    Read {
        #[command(flatten)]
        connector: ConnectorArgs,
        #[arg(long)]
        config: PathBuf,
        /// Configured catalog file; omit with --full-refresh to sync all
        /// discovered streams
        #[arg(long)]
        catalog: Option<PathBuf>,
        /// Discover and sync every stream in full-refresh/append mode
        #[arg(long, conflicts_with = "catalog")]
        full_refresh: bool,
        /// State file (JSON list of state messages) to resume from
        #[arg(long)]
        state: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command {
        Command::Spec { connector } => {
            let spec = connector.runner()?.spec().await?;
            println!("{}", serde_json::to_string_pretty(&spec)?);
        }
        Command::Check { connector, config } => {
            let status = connector.runner()?.check(&config).await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Command::Discover { connector, config } => {
            let catalog = connector.runner()?.discover(&config).await?;
            println!("{}", serde_json::to_string_pretty(&catalog)?);
        }
        Command::Read {
            connector,
            config,
            catalog,
            full_refresh,
            state,
        } => {
            let runner = connector.runner()?;

            // Stage an all-streams catalog when the user asked for
            // --full-refresh instead of providing one.
            let _staging = tempfile::tempdir().context("creating staging dir")?;
            let catalog_path = match (catalog, full_refresh) {
                (Some(path), _) => path,
                (None, true) => {
                    let discovered = runner.discover(&config).await?;
                    let configured = ConfiguredAirbyteCatalog {
                        streams: discovered
                            .streams
                            .into_iter()
                            .map(ConfiguredAirbyteStream::full_refresh)
                            .collect(),
                    };
                    let path = _staging.path().join("catalog.json");
                    std::fs::write(&path, serde_json::to_vec(&configured)?)?;
                    path
                }
                (None, false) => bail!("provide --catalog or use --full-refresh"),
            };

            // Records are the data plane: NDJSON on stdout. Stdout may close
            // early (`gauss read … | head`); stop writing instead of panicking.
            let mut stdout = std::io::stdout().lock();
            let mut stdout_open = true;
            let summary = runner
                .read(
                    &config,
                    &catalog_path,
                    state.as_deref(),
                    |event| match event {
                        ReadEvent::Message(msg) => {
                            if let Some(record) = &msg.record {
                                if stdout_open {
                                    let line =
                                        serde_json::to_string(record).expect("record serializes");
                                    stdout_open = writeln!(stdout, "{line}").is_ok();
                                }
                            } else if let Some(log) = &msg.log {
                                tracing::info!(target: "connector", "{}", log.message);
                            } else if let Some(trace) = &msg.trace {
                                tracing::debug!(target: "connector", ?trace, "trace");
                            }
                        }
                        ReadEvent::Raw(line) => {
                            tracing::info!(target: "connector_raw", "{line}");
                        }
                    },
                )
                .await?;

            eprintln!(
                "read complete: {} records, {} state checkpoints{}",
                summary.records,
                summary.state_messages,
                summary
                    .last_state
                    .as_ref()
                    .and_then(|s| serde_json::to_string(s).ok())
                    .map(|s| format!(", last state: {s}"))
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}

//! The connector binary runner: standard argument handling, wire output,
//! error-to-trace conversion, and exit codes.
//!
//! Behavior contract (what platforms expect of well-behaved connectors):
//! - `spec` always succeeds and prints a SPEC message.
//! - `check` never exits non-zero for user-fixable problems: connector
//!   errors become a FAILED `CONNECTION_STATUS` message.
//! - `discover`/`read`/`write` emit an ERROR trace and exit 1 on failure.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use gauss_protocol::*;
use serde_json::Value;

use crate::{CdkError, Destination, Emitter, Source};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Spec,
    Check {
        #[arg(long)]
        config: PathBuf,
    },
    Discover {
        #[arg(long)]
        config: PathBuf,
    },
    Read {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        state: Option<PathBuf>,
    },
    Write {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
    },
}

pub async fn run_source(source: impl Source) -> ExitCode {
    run(Some(&source), None::<&NoDestination>).await
}

pub async fn run_destination(destination: impl Destination) -> ExitCode {
    run(None::<&NoSource>, Some(&destination)).await
}

/// One binary acting as both a source and a destination (e.g. reference and
/// loopback connectors).
pub async fn run_dual(source: impl Source, destination: impl Destination) -> ExitCode {
    run(Some(&source), Some(&destination)).await
}

// Placeholder impls so `run` can be called with one side missing.
struct NoSource;
struct NoDestination;

#[async_trait::async_trait]
impl Source for NoSource {
    fn spec(&self) -> ConnectorSpecification {
        unreachable!()
    }
    async fn check(&self, _: &Value) -> Result<AirbyteConnectionStatus, CdkError> {
        unreachable!()
    }
    async fn discover(&self, _: &Value) -> Result<AirbyteCatalog, CdkError> {
        unreachable!()
    }
    async fn read(
        &self,
        _: &Value,
        _: &ConfiguredAirbyteCatalog,
        _: Option<&Value>,
        _: &mut Emitter,
    ) -> Result<(), CdkError> {
        unreachable!()
    }
}

#[async_trait::async_trait]
impl Destination for NoDestination {
    fn spec(&self) -> ConnectorSpecification {
        unreachable!()
    }
    async fn check(&self, _: &Value) -> Result<AirbyteConnectionStatus, CdkError> {
        unreachable!()
    }
    async fn write(
        &self,
        _: &Value,
        _: &ConfiguredAirbyteCatalog,
        _: &mut (dyn Iterator<Item = AirbyteMessage> + Send),
        _: &mut Emitter,
    ) -> Result<(), CdkError> {
        unreachable!()
    }
}

fn load_json(path: &Path) -> Result<Value, CdkError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| CdkError::Config(format!("reading {}: {e}", path.display())))?;
    serde_json::from_str(&raw)
        .map_err(|e| CdkError::Config(format!("parsing {}: {e}", path.display())))
}

fn load_catalog(path: &Path) -> Result<ConfiguredAirbyteCatalog, CdkError> {
    serde_json::from_value(load_json(path)?)
        .map_err(|e| CdkError::Config(format!("invalid configured catalog: {e}")))
}

async fn run(source: Option<&impl Source>, destination: Option<&impl Destination>) -> ExitCode {
    let mut emitter = Emitter::stdout();
    let result = dispatch(source, destination, &mut emitter).await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = emitter.error_trace(&error.to_string(), error.failure_type());
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(
    source: Option<&impl Source>,
    destination: Option<&impl Destination>,
    emitter: &mut Emitter,
) -> Result<(), CdkError> {
    let missing = |role: &str| CdkError::Config(format!("this connector has no {role} side"));

    match Cli::parse().command {
        Command::Spec => {
            // Dual connectors expose the source spec (sides share config).
            let spec = match (source, destination) {
                (Some(source), _) => source.spec(),
                (None, Some(destination)) => destination.spec(),
                (None, None) => unreachable!("runner always gets at least one side"),
            };
            emitter.message(&AirbyteMessage::spec(spec))
        }
        Command::Check { config } => {
            let config = load_json(&config)?;
            let checked = match (source, destination) {
                (Some(source), _) => source.check(&config).await,
                (None, Some(destination)) => destination.check(&config).await,
                (None, None) => unreachable!(),
            };
            // Connector-level failures are a FAILED status, not a crash.
            let status = checked.unwrap_or_else(|error| AirbyteConnectionStatus {
                status: ConnectionStatus::Failed,
                message: Some(error.to_string()),
            });
            emitter.message(&AirbyteMessage::connection_status(status))
        }
        Command::Discover { config } => {
            let source = source.ok_or_else(|| missing("source"))?;
            let catalog = source.discover(&load_json(&config)?).await?;
            emitter.message(&AirbyteMessage::catalog(catalog))
        }
        Command::Read {
            config,
            catalog,
            state,
        } => {
            let source = source.ok_or_else(|| missing("source"))?;
            let state = state.as_deref().map(load_json).transpose()?;
            source
                .read(
                    &load_json(&config)?,
                    &load_catalog(&catalog)?,
                    state.as_ref(),
                    emitter,
                )
                .await
        }
        Command::Write { config, catalog } => {
            let destination = destination.ok_or_else(|| missing("destination"))?;
            // Stdin is read on a dedicated thread (its lock isn't Send) and
            // handed to the async write as a channel-backed iterator.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let stdin = std::io::stdin();
                for line in std::io::BufRead::lines(stdin.lock()).map_while(Result::ok) {
                    if let Ok(message) = parse_message(&line) {
                        if tx.send(message).is_err() {
                            break;
                        }
                    }
                }
            });
            let mut messages = rx.into_iter();
            destination
                .write(
                    &load_json(&config)?,
                    &load_catalog(&catalog)?,
                    &mut messages,
                    emitter,
                )
                .await
        }
    }
}

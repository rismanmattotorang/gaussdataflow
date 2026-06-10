//! A protocol-complete Airbyte *source* connector written in Rust.
//!
//! Emits deterministic fake `users` records. It exists to prove the
//! gaussdataflow runtime end-to-end without Docker, and is the seed of the
//! Phase-5 Rust CDK: the `spec/check/discover/read` shape here is what the
//! CDK will extract into traits.
//!
//! Config: `{"record_count": <n>, "fail_check": <bool>}`.
//! State:  `{"cursor": <last emitted id>}` (per-stream, resumable).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use gauss_protocol::*;
use serde::Deserialize;
use serde_json::json;

const STREAM_NAME: &str = "users";

#[derive(Parser)]
#[command(name = "gauss-mock-connector")]
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
}

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_record_count")]
    record_count: u64,
    #[serde(default)]
    fail_check: bool,
}

fn default_record_count() -> u64 {
    10
}

#[derive(Deserialize, Default)]
struct StreamCursor {
    #[serde(default)]
    cursor: u64,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Spec => spec(),
        Command::Check { config } => check(&load_config(&config)?),
        Command::Discover { config } => {
            load_config(&config)?;
            discover()
        }
        Command::Read {
            config,
            catalog,
            state,
        } => read(&load_config(&config)?, &catalog, state.as_deref()),
    }
}

fn emit(message: &AirbyteMessage) -> Result<()> {
    println!("{}", to_wire(message)?);
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis() as i64
}

fn load_config(path: &std::path::Path) -> Result<Config> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).context("parsing config")
}

fn spec() -> Result<()> {
    let mut spec = ConnectorSpecification::new(json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "Mock Source Spec",
        "type": "object",
        "properties": {
            "record_count": {
                "type": "integer",
                "minimum": 0,
                "default": 10,
                "description": "Number of user records to emit"
            },
            "fail_check": {
                "type": "boolean",
                "default": false,
                "description": "Force `check` to fail (for testing)"
            }
        }
    }));
    spec.documentation_url = Some("https://github.com/rismanmattotorang/gaussdataflow".into());
    spec.supports_incremental = Some(true);
    spec.protocol_version = Some("0.2.0".into());
    emit(&AirbyteMessage::spec(spec))
}

fn check(config: &Config) -> Result<()> {
    let status = if config.fail_check {
        AirbyteConnectionStatus {
            status: ConnectionStatus::Failed,
            message: Some("fail_check was set".into()),
        }
    } else {
        AirbyteConnectionStatus {
            status: ConnectionStatus::Succeeded,
            message: None,
        }
    };
    emit(&AirbyteMessage::connection_status(status))
}

fn users_stream() -> AirbyteStream {
    let mut stream = AirbyteStream::new(
        STREAM_NAME,
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "name": {"type": "string"},
                "email": {"type": "string"},
                "created_at": {"type": "string", "format": "date-time"}
            }
        }),
    );
    stream.supported_sync_modes = Some(vec![SyncMode::FullRefresh, SyncMode::Incremental]);
    stream.source_defined_cursor = Some(true);
    stream.default_cursor_field = Some(vec!["id".into()]);
    stream.source_defined_primary_key = Some(vec![vec!["id".into()]]);
    stream.is_resumable = Some(true);
    stream
}

fn discover() -> Result<()> {
    emit(&AirbyteMessage::catalog(AirbyteCatalog {
        streams: vec![users_stream()],
    }))
}

fn read(
    config: &Config,
    catalog_path: &std::path::Path,
    state_path: Option<&std::path::Path>,
) -> Result<()> {
    let catalog: ConfiguredAirbyteCatalog =
        serde_json::from_str(&fs::read_to_string(catalog_path).context("reading catalog")?)
            .context("parsing configured catalog")?;
    let selected = catalog.streams.iter().any(|s| s.stream.name == STREAM_NAME);
    if !selected {
        // Nothing configured for us; a well-behaved source emits nothing.
        return Ok(());
    }

    // Resume from the per-stream cursor if state was provided.
    let mut cursor = 0u64;
    if let Some(path) = state_path {
        let messages: Vec<AirbyteStateMessage> =
            serde_json::from_str(&fs::read_to_string(path).context("reading state")?)
                .context("parsing state (expected a JSON list of state messages)")?;
        for state in messages {
            if let Some(stream) = state.stream {
                if stream.stream_descriptor.name == STREAM_NAME {
                    let parsed: StreamCursor = stream
                        .stream_state
                        .map(serde_json::from_value)
                        .transpose()?
                        .unwrap_or_default();
                    cursor = parsed.cursor;
                }
            }
        }
    }

    let descriptor = StreamDescriptor::new(STREAM_NAME);
    emit(&AirbyteMessage::trace(AirbyteTraceMessage::stream_status(
        now_millis() as f64,
        AirbyteStreamStatusTraceMessage {
            stream_descriptor: descriptor.clone(),
            status: StreamStatus::Started,
            reasons: None,
        },
    )))?;

    let mut emitted = 0u64;
    for id in (cursor + 1)..=config.record_count {
        emit(&AirbyteMessage::record(AirbyteRecordMessage {
            namespace: None,
            stream: STREAM_NAME.into(),
            data: json!({
                "id": id,
                "name": format!("user-{id}"),
                "email": format!("user-{id}@example.com"),
                "created_at": "2026-01-01T00:00:00Z"
            }),
            emitted_at: now_millis(),
            meta: None,
        }))?;
        emitted += 1;

        // Checkpoint every 5 records and at the end, like a real source.
        if id % 5 == 0 || id == config.record_count {
            let mut state = AirbyteStateMessage::stream(AirbyteStreamState {
                stream_descriptor: descriptor.clone(),
                stream_state: Some(json!({ "cursor": id })),
            });
            state.source_stats = Some(AirbyteStateStats {
                record_count: Some(emitted as f64),
            });
            emit(&AirbyteMessage::state(state))?;
            emitted = 0;
        }
    }

    emit(&AirbyteMessage::trace(AirbyteTraceMessage::stream_status(
        now_millis() as f64,
        AirbyteStreamStatusTraceMessage {
            stream_descriptor: descriptor,
            status: StreamStatus::Complete,
            reasons: None,
        },
    )))
}

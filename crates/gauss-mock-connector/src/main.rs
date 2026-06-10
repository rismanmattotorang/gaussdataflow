//! A protocol-complete Airbyte connector written in Rust — a *source*
//! (`read`) and, for replication testing, also a *destination* (`write`).
//!
//! Emits deterministic fake `users` records. It exists to prove the
//! gaussdataflow runtime end-to-end without Docker, and is the seed of the
//! Phase-5 Rust CDK: the `spec/check/discover/read/write` shape here is what
//! the CDK will extract into traits.
//!
//! Config: `{"record_count": <n>, "fail_check": <bool>,
//!           "emit_delay_ms": <ms per record>, "out_path": <file>}`.
//! State:  `{"cursor": <last emitted id>}` (per-stream, resumable).
//! As a destination it appends record `data` lines to `out_path` and acks
//! every STATE message back on stdout after flushing, like a real
//! destination.

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
    Write {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
    },
}

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_record_count")]
    record_count: u64,
    #[serde(default)]
    fail_check: bool,
    /// Sleep per emitted record; makes cancellation tests deterministic.
    #[serde(default)]
    emit_delay_ms: u64,
    /// Destination mode: file to append record data lines to.
    #[serde(default)]
    out_path: Option<PathBuf>,
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
        Command::Write { config, catalog } => {
            let _ = catalog; // accepted for protocol parity; all streams written
            write(&load_config(&config)?)
        }
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
        if config.emit_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(config.emit_delay_ms));
        }
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

/// Destination mode: consume protocol messages on stdin. Records are
/// appended (their `data`, one JSON line each) to `out_path` if set; every
/// STATE message is acked back on stdout *after* flushing what preceded it —
/// the contract replication workers rely on for checkpointing.
fn write(config: &Config) -> Result<()> {
    use std::io::{BufRead, Write as _};

    let mut out = match &config.out_path {
        Some(path) => Some(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("opening {}", path.display()))?,
        ),
        None => None,
    };

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = parse_message(&line) else {
            continue; // tolerate non-protocol lines like a real destination
        };
        if let Some(record) = &message.record {
            if let Some(file) = &mut out {
                writeln!(file, "{}", record.data).context("writing record")?;
            }
        }
        if message.state.is_some() {
            if let Some(file) = &mut out {
                file.flush().context("flushing before state ack")?;
            }
            emit(&message)?;
        }
    }
    Ok(())
}

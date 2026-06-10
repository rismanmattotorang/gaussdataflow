//! The reference connector, built on `gauss-cdk` — a *source* (deterministic
//! fake `users` records, resumable incremental cursor) and a *destination*
//! (appends record data to a file, acks states after flushing) in one
//! binary. It powers the platform's hermetic e2e suite and doubles as the
//! canonical example of writing connectors with the CDK.
//!
//! Config: `{"record_count": <n>, "fail_check": <bool>,
//!           "emit_delay_ms": <ms per record>, "out_path": <file>}`.
//! State:  `{"cursor": <last emitted id>}` (per-stream, resumable).

use std::io::Write as _;

use gauss_cdk::protocol::*;
use gauss_cdk::{CdkError, Destination, Emitter, Source};
use serde::Deserialize;
use serde_json::{json, Value};

const STREAM_NAME: &str = "users";

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
    out_path: Option<std::path::PathBuf>,
}

fn default_record_count() -> u64 {
    10
}

fn parse_config(config: &Value) -> Result<Config, CdkError> {
    serde_json::from_value(config.clone())
        .map_err(|e| CdkError::Config(format!("invalid config: {e}")))
}

fn spec() -> ConnectorSpecification {
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
            },
            "emit_delay_ms": {
                "type": "integer",
                "minimum": 0,
                "default": 0,
                "description": "Sleep per record (for cancellation testing)"
            },
            "out_path": {
                "type": "string",
                "description": "Destination mode: file to append records to"
            }
        }
    }));
    spec.documentation_url = Some("https://github.com/rismanmattotorang/gaussdataflow".into());
    spec.supports_incremental = Some(true);
    spec.protocol_version = Some("0.2.0".into());
    spec
}

struct MockSource;

#[async_trait::async_trait]
impl Source for MockSource {
    fn spec(&self) -> ConnectorSpecification {
        spec()
    }

    async fn check(&self, config: &Value) -> Result<GaussConnectionStatus, CdkError> {
        let config = parse_config(config)?;
        Ok(if config.fail_check {
            GaussConnectionStatus {
                status: ConnectionStatus::Failed,
                message: Some("fail_check was set".into()),
            }
        } else {
            GaussConnectionStatus {
                status: ConnectionStatus::Succeeded,
                message: None,
            }
        })
    }

    async fn discover(&self, config: &Value) -> Result<GaussCatalog, CdkError> {
        parse_config(config)?;
        let mut stream = GaussStream::new(
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
        Ok(GaussCatalog {
            streams: vec![stream],
        })
    }

    async fn read(
        &self,
        config: &Value,
        catalog: &ConfiguredGaussCatalog,
        state: Option<&Value>,
        emitter: &mut Emitter,
    ) -> Result<(), CdkError> {
        let config = parse_config(config)?;
        let selected = catalog.streams.iter().any(|s| s.stream.name == STREAM_NAME);
        if !selected {
            // Nothing configured for us; a well-behaved source emits nothing.
            return Ok(());
        }

        // Resume from the per-stream cursor if state was provided.
        let cursor = gauss_cdk::state::cursor_value(state, STREAM_NAME, "cursor")
            .and_then(Value::as_u64)
            .unwrap_or(0);

        emitter.stream_status(STREAM_NAME, StreamStatus::Started)?;

        let mut emitted = 0u64;
        for id in (cursor + 1)..=config.record_count {
            if config.emit_delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(config.emit_delay_ms));
            }
            emitter.record(
                STREAM_NAME,
                None,
                json!({
                    "id": id,
                    "name": format!("user-{id}"),
                    "email": format!("user-{id}@example.com"),
                    "created_at": "2026-01-01T00:00:00Z"
                }),
            )?;
            emitted += 1;

            // Checkpoint every 5 records and at the end, like a real source.
            if id % 5 == 0 || id == config.record_count {
                emitter.stream_state(STREAM_NAME, json!({ "cursor": id }), Some(emitted as f64))?;
                emitted = 0;
            }
        }

        emitter.stream_status(STREAM_NAME, StreamStatus::Complete)
    }
}

struct MockDestination;

#[async_trait::async_trait]
impl Destination for MockDestination {
    fn spec(&self) -> ConnectorSpecification {
        spec()
    }

    async fn check(&self, config: &Value) -> Result<GaussConnectionStatus, CdkError> {
        MockSource.check(config).await
    }

    async fn write(
        &self,
        config: &Value,
        _catalog: &ConfiguredGaussCatalog,
        messages: &mut (dyn Iterator<Item = GaussMessage> + Send),
        emitter: &mut Emitter,
    ) -> Result<(), CdkError> {
        let config = parse_config(config)?;
        let mut out = match &config.out_path {
            Some(path) => Some(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| CdkError::Config(format!("opening {}: {e}", path.display())))?,
            ),
            None => None,
        };

        for message in messages {
            if let Some(record) = &message.record {
                if let Some(file) = &mut out {
                    writeln!(file, "{}", record.data)?;
                }
            }
            if message.state.is_some() {
                // Ack only after flushing everything before this state.
                if let Some(file) = &mut out {
                    file.flush()?;
                }
                emitter.message(&message)?;
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    gauss_cdk::cli::run_dual(MockSource, MockDestination).await
}

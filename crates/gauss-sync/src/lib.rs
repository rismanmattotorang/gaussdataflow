//! The replication worker: pipes a source connector's output into a
//! destination connector, checkpointing state along the way.
//!
//! Dataflow (all newline-delimited protocol JSON):
//!
//! ```text
//! source `read` stdout ──RECORD/STATE──▶ destination `write` stdin
//! destination stdout ──STATE (committed)──▶ checkpoint callback → persisted
//! ```
//!
//! Key properties:
//! - **Backpressure** comes from the OS pipe: writing to the destination's
//!   stdin suspends when it falls behind. The destination's *stdout* is
//!   drained on an independent task (unbounded, control-plane volume only)
//!   so a slow checkpoint path can never deadlock the record path.
//! - **Checkpoints are destination-acked**: a STATE message only reaches the
//!   checkpoint callback after the destination has emitted it back, i.e.
//!   after it durably flushed everything before it. Crash-resume never skips
//!   data.
//! - **Cancellation** is a watch channel checked between messages; child
//!   processes are killed on drop.
//! - **Idle timeout** bounds how long the sync waits for either connector to
//!   produce a line.

use std::collections::BTreeMap;
use std::time::Duration;

use gauss_connector_runtime::{
    ConnectorCommand, ConnectorOutput, ConnectorProcess, Launcher, RuntimeError,
};
use gauss_protocol::{GaussMessage, GaussMessageType, GaussTraceType, StreamStatus};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("source failed: {0}")]
    SourceFailed(String),
    #[error("destination failed: {0}")]
    DestinationFailed(String),
    #[error("sync cancelled")]
    Cancelled,
    #[error("no output from connectors for {0:?}")]
    IdleTimeout(Duration),
    #[error("checkpoint persistence failed: {0}")]
    Checkpoint(String),
}

pub struct SyncRequest {
    pub source_config: Value,
    pub destination_config: Value,
    /// ConfiguredGaussCatalog wire form.
    pub catalog: Value,
    /// JSON array of GaussStateMessage wire forms to resume from.
    pub state: Option<Value>,
}

pub struct SyncOptions {
    pub idle_timeout: Duration,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Default)]
pub struct SyncSummary {
    pub records_synced: u64,
    pub bytes_synced: u64,
    pub committed_states: u64,
    /// Destination-acked state, merged per stream: a JSON array of state
    /// messages suitable as the next sync's `state` input.
    pub final_state: Option<Value>,
    /// Last observed source stream status per stream name.
    pub stream_statuses: BTreeMap<String, String>,
}

/// Merge key for a state message: one slot per stream (or one global/legacy
/// slot), so the persisted state is always "latest per stream".
pub fn state_key(state: &Value) -> String {
    match state.get("type").and_then(Value::as_str) {
        Some("GLOBAL") => "__global".to_string(),
        None => "__legacy".to_string(),
        _ => {
            let descriptor = &state["stream"]["stream_descriptor"];
            format!(
                "{}\u{1f}{}",
                descriptor["namespace"].as_str().unwrap_or(""),
                descriptor["name"].as_str().unwrap_or("")
            )
        }
    }
}

/// Run one replication: source `read` piped into destination `write`.
///
/// `on_checkpoint` is invoked with each destination-acked state message
/// (wire form); persist it before returning `Ok` — a returned error aborts
/// the sync.
pub async fn run_sync<F, Fut>(
    source: &dyn Launcher,
    destination: &dyn Launcher,
    request: &SyncRequest,
    options: &SyncOptions,
    mut cancel: watch::Receiver<bool>,
    mut on_checkpoint: F,
) -> Result<SyncSummary, SyncError>
where
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    // Stage connector input files.
    let staging = tempfile::tempdir()?;
    let stage = |name: &str, value: &Value| -> Result<std::path::PathBuf, SyncError> {
        let path = staging.path().join(name);
        std::fs::write(&path, serde_json::to_vec(value)?)?;
        Ok(path)
    };
    let src_config = stage("source_config.json", &request.source_config)?;
    let dst_config = stage("destination_config.json", &request.destination_config)?;
    let catalog = stage("catalog.json", &request.catalog)?;
    let state = match &request.state {
        Some(value) => Some(stage("state.json", value)?),
        None => None,
    };

    let mut src = ConnectorProcess::spawn(
        source,
        &ConnectorCommand::Read {
            config: src_config,
            catalog: catalog.clone(),
            state,
        },
    )?;
    let mut dst = ConnectorProcess::spawn(
        destination,
        &ConnectorCommand::Write {
            config: dst_config,
            catalog,
        },
    )?;
    let mut dst_stdin = dst
        .stdin()
        .ok_or_else(|| SyncError::DestinationFailed("destination stdin unavailable".into()))?;

    // Drain destination stdout independently so the record path can never
    // deadlock against the ack path. Control-plane volume only → unbounded.
    let (dst_tx, mut dst_rx) = mpsc::unbounded_channel::<Result<ConnectorOutput, RuntimeError>>();
    let dst_task = tokio::spawn(async move {
        loop {
            match dst.next().await {
                Ok(Some(output)) => {
                    if dst_tx.send(Ok(output)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    let _ = dst_tx.send(Err(err));
                    break;
                }
            }
        }
        dst.wait().await
    });

    let mut summary = SyncSummary::default();
    let mut state_map: BTreeMap<String, Value> = match &request.state {
        Some(Value::Array(messages)) => {
            messages.iter().map(|m| (state_key(m), m.clone())).collect()
        }
        _ => BTreeMap::new(),
    };
    let mut source_error: Option<String> = None;
    let mut destination_error: Option<String> = None;

    macro_rules! handle_dest {
        ($output:expr) => {
            if let ConnectorOutput::Message(msg) = $output {
                if let Some(state) = &msg.state {
                    let value = serde_json::to_value(state)?;
                    on_checkpoint(value.clone())
                        .await
                        .map_err(SyncError::Checkpoint)?;
                    state_map.insert(state_key(&value), value);
                    summary.committed_states += 1;
                }
                if let Some(err) = extract_error(&msg) {
                    destination_error = Some(err);
                }
            }
        };
    }

    // Main pump: source stdout → destination stdin.
    loop {
        if *cancel.borrow() {
            return Err(SyncError::Cancelled);
        }
        while let Ok(output) = dst_rx.try_recv() {
            handle_dest!(output?);
        }

        let next = tokio::select! {
            biased;
            _ = cancel.changed() => return Err(SyncError::Cancelled),
            next = timeout(options.idle_timeout, src.next()) => {
                next.map_err(|_| SyncError::IdleTimeout(options.idle_timeout))??
            }
        };
        let Some(output) = next else {
            break; // source EOF
        };
        let ConnectorOutput::Message(msg) = output else {
            continue; // non-protocol line, already logged by the runtime
        };

        match msg.message_type {
            GaussMessageType::Record | GaussMessageType::State => {
                let line = gauss_protocol::to_wire(&msg)?;
                let write = async {
                    dst_stdin.write_all(line.as_bytes()).await?;
                    dst_stdin.write_all(b"\n").await?;
                    Ok::<_, std::io::Error>(())
                };
                tokio::select! {
                    biased;
                    _ = cancel.changed() => return Err(SyncError::Cancelled),
                    result = timeout(options.idle_timeout, write) => {
                        result.map_err(|_| SyncError::IdleTimeout(options.idle_timeout))??;
                    }
                }
                if msg.message_type == GaussMessageType::Record {
                    summary.records_synced += 1;
                    summary.bytes_synced += line.len() as u64 + 1;
                }
            }
            GaussMessageType::Trace => {
                if let Some(err) = extract_error(&msg) {
                    source_error = Some(err);
                }
                if let Some(trace) = &msg.trace {
                    if trace.trace_type == GaussTraceType::StreamStatus {
                        if let Some(status) = &trace.stream_status {
                            summary.stream_statuses.insert(
                                status.stream_descriptor.name.clone(),
                                stream_status_str(status.status).to_string(),
                            );
                        }
                    }
                }
            }
            GaussMessageType::Log => {
                if let Some(log) = &msg.log {
                    tracing::info!(target: "sync_source", "{}", log.message);
                }
            }
            _ => {}
        }
    }

    // Source finished: verify it, close the destination's stdin, and drain
    // the remaining destination acks.
    let src_status = src.wait().await?;
    if !src_status.success() {
        return Err(SyncError::SourceFailed(
            source_error.unwrap_or_else(|| format!("exit code {:?}", src_status.code())),
        ));
    }
    drop(dst_stdin);

    loop {
        let received = tokio::select! {
            biased;
            _ = cancel.changed() => return Err(SyncError::Cancelled),
            received = timeout(options.idle_timeout, dst_rx.recv()) => {
                received.map_err(|_| SyncError::IdleTimeout(options.idle_timeout))?
            }
        };
        match received {
            Some(output) => handle_dest!(output?),
            None => break,
        }
    }

    let dst_status = dst_task
        .await
        .map_err(|err| SyncError::DestinationFailed(err.to_string()))??;
    if !dst_status.success() {
        return Err(SyncError::DestinationFailed(
            destination_error.unwrap_or_else(|| format!("exit code {:?}", dst_status.code())),
        ));
    }

    if !state_map.is_empty() {
        summary.final_state = Some(Value::Array(state_map.into_values().collect()));
    }
    Ok(summary)
}

fn extract_error(msg: &GaussMessage) -> Option<String> {
    let trace = msg.trace.as_ref()?;
    if trace.trace_type == GaussTraceType::Error {
        Some(
            trace
                .error
                .as_ref()
                .map(|e| e.message.clone())
                .unwrap_or_else(|| "unknown connector error".to_string()),
        )
    } else {
        None
    }
}

fn stream_status_str(status: StreamStatus) -> &'static str {
    match status {
        StreamStatus::Started => "started",
        StreamStatus::Running => "running",
        StreamStatus::Complete => "complete",
        StreamStatus::Incomplete => "incomplete",
    }
}

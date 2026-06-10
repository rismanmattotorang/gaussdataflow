use std::path::{Path, PathBuf};

use gauss_protocol::{
    AirbyteCatalog, AirbyteConnectionStatus, AirbyteErrorTraceMessage, AirbyteMessage,
    AirbyteStateMessage, AirbyteTraceType, ConnectorSpecification,
};

use crate::error::RuntimeError;
use crate::launcher::{ConnectorCommand, Launcher};
use crate::process::{ConnectorOutput, ConnectorProcess};

/// High-level driver for the standard connector operations.
pub struct ConnectorRunner {
    launcher: Box<dyn Launcher>,
}

/// One event surfaced to the caller during `read`.
#[derive(Debug)]
pub enum ReadEvent {
    Message(Box<AirbyteMessage>),
    /// A non-protocol STDOUT line.
    Raw(String),
}

#[derive(Debug, Default)]
pub struct ReadSummary {
    pub records: u64,
    pub state_messages: u64,
    /// Latest checkpoint — what a platform would persist to resume the sync.
    pub last_state: Option<AirbyteStateMessage>,
}

impl ConnectorRunner {
    pub fn new(launcher: impl Launcher + 'static) -> Self {
        Self {
            launcher: Box::new(launcher),
        }
    }

    pub async fn spec(&self) -> Result<ConnectorSpecification, RuntimeError> {
        self.first(ConnectorCommand::Spec, "SPEC", |m| m.spec).await
    }

    pub async fn check(&self, config: &Path) -> Result<AirbyteConnectionStatus, RuntimeError> {
        let cmd = ConnectorCommand::Check {
            config: config.to_path_buf(),
        };
        self.first(cmd, "CONNECTION_STATUS", |m| m.connection_status)
            .await
    }

    pub async fn discover(&self, config: &Path) -> Result<AirbyteCatalog, RuntimeError> {
        let cmd = ConnectorCommand::Discover {
            config: config.to_path_buf(),
        };
        self.first(cmd, "CATALOG", |m| m.catalog).await
    }

    /// Run a source `read`, invoking `on_event` for every output line.
    pub async fn read(
        &self,
        config: &Path,
        catalog: &Path,
        state: Option<&Path>,
        mut on_event: impl FnMut(ReadEvent),
    ) -> Result<ReadSummary, RuntimeError> {
        let cmd = ConnectorCommand::Read {
            config: config.to_path_buf(),
            catalog: catalog.to_path_buf(),
            state: state.map(Path::to_path_buf),
        };
        let mut process = ConnectorProcess::spawn(self.launcher.as_ref(), &cmd)?;

        let mut summary = ReadSummary::default();
        let mut error: Option<AirbyteErrorTraceMessage> = None;

        while let Some(output) = process.next().await? {
            match output {
                ConnectorOutput::Message(msg) => {
                    if msg.record.is_some() {
                        summary.records += 1;
                    }
                    if let Some(state) = &msg.state {
                        summary.state_messages += 1;
                        summary.last_state = Some(state.clone());
                    }
                    if let Some(err) = extract_error(&msg) {
                        error = Some(err);
                    }
                    on_event(ReadEvent::Message(msg));
                }
                ConnectorOutput::Raw(line) => on_event(ReadEvent::Raw(line)),
            }
        }

        let status = process.wait().await?;
        if let Some(err) = error {
            return Err(RuntimeError::ConnectorError(Box::new(err)));
        }
        if !status.success() {
            return Err(RuntimeError::Failed {
                exit_code: status.code(),
            });
        }
        Ok(summary)
    }

    /// Run an operation and return the first message payload selected by
    /// `extract`, draining remaining output afterwards.
    async fn first<T>(
        &self,
        cmd: ConnectorCommand,
        expected: &'static str,
        extract: impl Fn(AirbyteMessage) -> Option<T>,
    ) -> Result<T, RuntimeError> {
        let mut process = ConnectorProcess::spawn(self.launcher.as_ref(), &cmd)?;
        let mut result: Option<T> = None;
        let mut error: Option<AirbyteErrorTraceMessage> = None;

        while let Some(output) = process.next().await? {
            if let ConnectorOutput::Message(msg) = output {
                if let Some(err) = extract_error(&msg) {
                    error = Some(err);
                }
                if result.is_none() {
                    result = extract(*msg);
                }
            }
        }
        let status = process.wait().await?;

        match (result, error) {
            (Some(value), _) => Ok(value),
            (None, Some(err)) => Err(RuntimeError::ConnectorError(Box::new(err))),
            (None, None) => Err(RuntimeError::MissingOutput {
                expected,
                exit_code: status.code(),
            }),
        }
    }
}

fn extract_error(msg: &AirbyteMessage) -> Option<AirbyteErrorTraceMessage> {
    let trace = msg.trace.as_ref()?;
    if trace.trace_type == AirbyteTraceType::Error {
        trace.error.clone()
    } else {
        None
    }
}

/// Convenience: write a JSON value to `dir/<name>.json` (connector inputs are
/// file-based; the platform stages them before launch).
pub async fn stage_json(
    dir: &Path,
    name: &str,
    value: &serde_json::Value,
) -> Result<PathBuf, RuntimeError> {
    let path = dir.join(format!("{name}.json"));
    tokio::fs::write(&path, serde_json::to_vec_pretty(value)?).await?;
    Ok(path)
}

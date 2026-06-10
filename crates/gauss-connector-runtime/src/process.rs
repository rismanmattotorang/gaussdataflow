use std::process::{ExitStatus, Stdio};

use gauss_protocol::GaussMessage;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::error::RuntimeError;
use crate::launcher::{ConnectorCommand, Launcher};

/// One line of connector STDOUT.
#[derive(Debug)]
pub enum ConnectorOutput {
    Message(Box<GaussMessage>),
    /// Anything that is not valid protocol JSON — connectors occasionally
    /// leak plain log lines; the platform must not crash on them.
    Raw(String),
}

/// A running connector with a typed view of its STDOUT.
pub struct ConnectorProcess {
    child: Child,
    stdout: Lines<BufReader<ChildStdout>>,
    program: String,
}

impl ConnectorProcess {
    pub fn spawn(
        launcher: &dyn Launcher,
        command: &ConnectorCommand,
    ) -> Result<Self, RuntimeError> {
        let mut cmd = launcher.build(command)?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let program = launcher.describe();
        let mut child = cmd.spawn().map_err(|source| RuntimeError::Spawn {
            program: program.clone(),
            source,
        })?;

        let stdout = BufReader::new(child.stdout.take().expect("stdout was piped")).lines();

        // STDERR carries connector diagnostics; forward it to our logs.
        let stderr = child.stderr.take().expect("stderr was piped");
        let stderr_program = program.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::warn!(target: "connector_stderr", connector = %stderr_program, "{line}");
            }
        });

        Ok(Self {
            child,
            stdout,
            program,
        })
    }

    /// Handle to the connector's STDIN (used by destination `write`).
    pub fn stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// Next line of output, parsed when possible. `None` at EOF.
    pub async fn next(&mut self) -> Result<Option<ConnectorOutput>, RuntimeError> {
        match self.stdout.next_line().await? {
            None => Ok(None),
            Some(line) if line.trim().is_empty() => Ok(Some(ConnectorOutput::Raw(line))),
            Some(line) => match gauss_protocol::parse_message(&line) {
                Ok(msg) => Ok(Some(ConnectorOutput::Message(Box::new(msg)))),
                Err(err) => {
                    tracing::debug!(connector = %self.program, %err, "non-protocol stdout line");
                    Ok(Some(ConnectorOutput::Raw(line)))
                }
            },
        }
    }

    /// Wait for the connector to exit (call after draining output).
    pub async fn wait(mut self) -> Result<ExitStatus, RuntimeError> {
        Ok(self.child.wait().await?)
    }
}

use std::path::PathBuf;

use gauss_protocol::GaussErrorTraceMessage;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("failed to spawn connector `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("i/o error while talking to connector: {0}")]
    Io(#[from] std::io::Error),

    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("cannot resolve path `{0}` (file must exist for container mounting)")]
    InvalidPath(PathBuf),

    #[error(
        "connector exited (code {exit_code:?}) without emitting the expected {expected} message"
    )]
    MissingOutput {
        expected: &'static str,
        exit_code: Option<i32>,
    },

    #[error("connector reported an error: {}", .0.message)]
    ConnectorError(Box<GaussErrorTraceMessage>),

    #[error("connector exited with failure code {exit_code:?}")]
    Failed { exit_code: Option<i32> },
}

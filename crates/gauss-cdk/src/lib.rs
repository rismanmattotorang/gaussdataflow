//! Connector Development Kit: build protocol-complete connectors in Rust.
//!
//! Implement [`Source`] (and/or [`Destination`]) and hand it to
//! [`cli::run_source`] / [`cli::run_destination`] / [`cli::run_dual`] — the
//! runner gives you the full connector binary: standard
//! `spec/check/discover/read/write` argument handling, wire-format output,
//! failure-to-trace conversion, and correct exit codes. The result runs
//! anywhere the platform launches connectors, container-free via the `exec:`
//! launcher scheme.
//!
//! ```ignore
//! struct MySource;
//!
//! #[async_trait::async_trait]
//! impl gauss_cdk::Source for MySource { /* spec/check/discover/read */ }
//!
//! #[tokio::main]
//! async fn main() -> std::process::ExitCode {
//!     gauss_cdk::cli::run_source(MySource).await
//! }
//! ```

pub mod cli;
mod emitter;
pub mod state;

pub use emitter::Emitter;
pub use gauss_protocol as protocol;

use gauss_protocol::{
    AirbyteCatalog, AirbyteConnectionStatus, ConfiguredAirbyteCatalog, ConnectorSpecification,
};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum CdkError {
    /// User-fixable configuration problems (bad credentials, missing field).
    #[error("config error: {0}")]
    Config(String),
    /// Transient upstream failures (network, rate limits) — retryable.
    #[error("transient error: {0}")]
    Transient(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl CdkError {
    pub fn failure_type(&self) -> gauss_protocol::FailureType {
        match self {
            Self::Config(_) => gauss_protocol::FailureType::ConfigError,
            Self::Transient(_) => gauss_protocol::FailureType::TransientError,
            _ => gauss_protocol::FailureType::SystemError,
        }
    }
}

/// A source connector: emits records from somewhere.
#[async_trait::async_trait]
pub trait Source: Send + Sync {
    fn spec(&self) -> ConnectorSpecification;

    async fn check(&self, config: &Value) -> Result<AirbyteConnectionStatus, CdkError>;

    async fn discover(&self, config: &Value) -> Result<AirbyteCatalog, CdkError>;

    /// Emit records (and state checkpoints) for the configured streams,
    /// resuming from `state` (a JSON array of state messages) when given.
    async fn read(
        &self,
        config: &Value,
        catalog: &ConfiguredAirbyteCatalog,
        state: Option<&Value>,
        emitter: &mut Emitter,
    ) -> Result<(), CdkError>;
}

/// A destination connector: consumes records from STDIN-shaped input.
#[async_trait::async_trait]
pub trait Destination: Send + Sync {
    fn spec(&self) -> ConnectorSpecification;

    async fn check(&self, config: &Value) -> Result<AirbyteConnectionStatus, CdkError>;

    /// Consume the incoming message stream. Contract: ack each STATE message
    /// (emit it back) only after everything before it is durably written.
    async fn write(
        &self,
        config: &Value,
        catalog: &ConfiguredAirbyteCatalog,
        messages: &mut (dyn Iterator<Item = gauss_protocol::AirbyteMessage> + Send),
        emitter: &mut Emitter,
    ) -> Result<(), CdkError>;
}

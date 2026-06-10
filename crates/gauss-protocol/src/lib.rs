//! Wire-exact Rust model of the Gauss Protocol (v0 series).
//!
//! Connectors are independent programs that emit newline-delimited JSON
//! [`GaussMessage`]s on STDOUT. This crate models those messages with
//! serde so that any Gauss-compatible connector can be driven from Rust.
//!
//! Field names and enum casings deliberately mirror the protocol's JSON
//! schemas (a mix of `snake_case`, `camelCase`, and `SCREAMING_SNAKE_CASE`).
//! Unknown fields are ignored on input and `None` fields are omitted on
//! output, so messages survive protocol evolution in both directions.

pub mod catalog;
pub mod message;
pub mod spec;
pub mod state;
pub mod trace;

pub use catalog::{
    ConfiguredGaussCatalog, ConfiguredGaussStream, DestinationSyncMode, GaussCatalog, GaussStream,
    SyncMode,
};
pub use message::{
    ConnectionStatus, GaussConnectionStatus, GaussControlConnectorConfigMessage,
    GaussControlMessage, GaussControlMessageType, GaussLogLevel, GaussLogMessage, GaussMessage,
    GaussMessageType, GaussRecordMessage,
};
pub use spec::ConnectorSpecification;
pub use state::{
    GaussGlobalState, GaussStateMessage, GaussStateStats, GaussStateType, GaussStreamState,
    StreamDescriptor,
};
pub use trace::{
    EstimateType, FailureType, GaussAnalyticsTraceMessage, GaussErrorTraceMessage,
    GaussEstimateTraceMessage, GaussStreamStatusTraceMessage, GaussTraceMessage, GaussTraceType,
    StreamStatus,
};

/// Parse a single line of connector STDOUT into a message.
pub fn parse_message(line: &str) -> Result<GaussMessage, serde_json::Error> {
    serde_json::from_str(line)
}

/// Serialize a message to its single-line wire form.
pub fn to_wire(message: &GaussMessage) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

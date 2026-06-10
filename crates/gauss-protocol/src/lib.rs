//! Wire-exact Rust model of the Airbyte Protocol (v0 series).
//!
//! Connectors are independent programs that emit newline-delimited JSON
//! [`AirbyteMessage`]s on STDOUT. This crate models those messages with
//! serde so that any Airbyte-compatible connector can be driven from Rust.
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
    AirbyteCatalog, AirbyteStream, ConfiguredAirbyteCatalog, ConfiguredAirbyteStream,
    DestinationSyncMode, SyncMode,
};
pub use message::{
    AirbyteConnectionStatus, AirbyteControlConnectorConfigMessage, AirbyteControlMessage,
    AirbyteControlMessageType, AirbyteLogLevel, AirbyteLogMessage, AirbyteMessage,
    AirbyteMessageType, AirbyteRecordMessage, ConnectionStatus,
};
pub use spec::ConnectorSpecification;
pub use state::{
    AirbyteGlobalState, AirbyteStateMessage, AirbyteStateStats, AirbyteStateType,
    AirbyteStreamState, StreamDescriptor,
};
pub use trace::{
    AirbyteAnalyticsTraceMessage, AirbyteErrorTraceMessage, AirbyteEstimateTraceMessage,
    AirbyteStreamStatusTraceMessage, AirbyteTraceMessage, AirbyteTraceType, EstimateType,
    FailureType, StreamStatus,
};

/// Parse a single line of connector STDOUT into a message.
pub fn parse_message(line: &str) -> Result<AirbyteMessage, serde_json::Error> {
    serde_json::from_str(line)
}

/// Serialize a message to its single-line wire form.
pub fn to_wire(message: &AirbyteMessage) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

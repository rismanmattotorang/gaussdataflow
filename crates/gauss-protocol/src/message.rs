//! The top-level [`AirbyteMessage`] envelope and its simple payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::AirbyteCatalog;
use crate::spec::ConnectorSpecification;
use crate::state::AirbyteStateMessage;
use crate::trace::AirbyteTraceMessage;

/// One newline-delimited JSON message on a connector's STDOUT/STDIN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteMessage {
    #[serde(rename = "type")]
    pub message_type: AirbyteMessageType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<AirbyteLogMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<ConnectorSpecification>,
    #[serde(rename = "connectionStatus", skip_serializing_if = "Option::is_none")]
    pub connection_status: Option<AirbyteConnectionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<AirbyteCatalog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<AirbyteRecordMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<AirbyteStateMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<AirbyteTraceMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<AirbyteControlMessage>,
}

impl AirbyteMessage {
    fn envelope(message_type: AirbyteMessageType) -> Self {
        Self {
            message_type,
            log: None,
            spec: None,
            connection_status: None,
            catalog: None,
            record: None,
            state: None,
            trace: None,
            control: None,
        }
    }

    pub fn record(record: AirbyteRecordMessage) -> Self {
        Self {
            record: Some(record),
            ..Self::envelope(AirbyteMessageType::Record)
        }
    }

    pub fn state(state: AirbyteStateMessage) -> Self {
        Self {
            state: Some(state),
            ..Self::envelope(AirbyteMessageType::State)
        }
    }

    pub fn log(log: AirbyteLogMessage) -> Self {
        Self {
            log: Some(log),
            ..Self::envelope(AirbyteMessageType::Log)
        }
    }

    pub fn spec(spec: ConnectorSpecification) -> Self {
        Self {
            spec: Some(spec),
            ..Self::envelope(AirbyteMessageType::Spec)
        }
    }

    pub fn connection_status(status: AirbyteConnectionStatus) -> Self {
        Self {
            connection_status: Some(status),
            ..Self::envelope(AirbyteMessageType::ConnectionStatus)
        }
    }

    pub fn catalog(catalog: AirbyteCatalog) -> Self {
        Self {
            catalog: Some(catalog),
            ..Self::envelope(AirbyteMessageType::Catalog)
        }
    }

    pub fn trace(trace: AirbyteTraceMessage) -> Self {
        Self {
            trace: Some(trace),
            ..Self::envelope(AirbyteMessageType::Trace)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AirbyteMessageType {
    Record,
    State,
    Log,
    Spec,
    ConnectionStatus,
    Catalog,
    Trace,
    Control,
}

/// A single data record emitted by a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteRecordMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub stream: String,
    pub data: Value,
    /// Epoch milliseconds at which the record was emitted.
    pub emitted_at: i64,
    /// Record metadata (e.g. per-field changes); schema still evolving
    /// upstream, kept opaque.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteLogMessage {
    pub level: AirbyteLogLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AirbyteLogLevel {
    Fatal,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Result of a `check` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteConnectionStatus {
    pub status: ConnectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectionStatus {
    Succeeded,
    Failed,
}

/// Out-of-band message from connector to platform (e.g. refreshed OAuth
/// config that the platform should persist).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteControlMessage {
    #[serde(rename = "type")]
    pub control_type: AirbyteControlMessageType,
    /// Epoch milliseconds.
    pub emitted_at: f64,
    #[serde(rename = "connectorConfig", skip_serializing_if = "Option::is_none")]
    pub connector_config: Option<AirbyteControlConnectorConfigMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AirbyteControlMessageType {
    ConnectorConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteControlConnectorConfigMessage {
    pub config: Value,
}

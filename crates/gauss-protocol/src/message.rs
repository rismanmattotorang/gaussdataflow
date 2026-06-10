//! The top-level [`GaussMessage`] envelope and its simple payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::GaussCatalog;
use crate::spec::ConnectorSpecification;
use crate::state::GaussStateMessage;
use crate::trace::GaussTraceMessage;

/// One newline-delimited JSON message on a connector's STDOUT/STDIN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussMessage {
    #[serde(rename = "type")]
    pub message_type: GaussMessageType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<GaussLogMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<ConnectorSpecification>,
    #[serde(rename = "connectionStatus", skip_serializing_if = "Option::is_none")]
    pub connection_status: Option<GaussConnectionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<GaussCatalog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<GaussRecordMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<GaussStateMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<GaussTraceMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<GaussControlMessage>,
}

impl GaussMessage {
    fn envelope(message_type: GaussMessageType) -> Self {
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

    pub fn record(record: GaussRecordMessage) -> Self {
        Self {
            record: Some(record),
            ..Self::envelope(GaussMessageType::Record)
        }
    }

    pub fn state(state: GaussStateMessage) -> Self {
        Self {
            state: Some(state),
            ..Self::envelope(GaussMessageType::State)
        }
    }

    pub fn log(log: GaussLogMessage) -> Self {
        Self {
            log: Some(log),
            ..Self::envelope(GaussMessageType::Log)
        }
    }

    pub fn spec(spec: ConnectorSpecification) -> Self {
        Self {
            spec: Some(spec),
            ..Self::envelope(GaussMessageType::Spec)
        }
    }

    pub fn connection_status(status: GaussConnectionStatus) -> Self {
        Self {
            connection_status: Some(status),
            ..Self::envelope(GaussMessageType::ConnectionStatus)
        }
    }

    pub fn catalog(catalog: GaussCatalog) -> Self {
        Self {
            catalog: Some(catalog),
            ..Self::envelope(GaussMessageType::Catalog)
        }
    }

    pub fn trace(trace: GaussTraceMessage) -> Self {
        Self {
            trace: Some(trace),
            ..Self::envelope(GaussMessageType::Trace)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GaussMessageType {
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
pub struct GaussRecordMessage {
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
pub struct GaussLogMessage {
    pub level: GaussLogLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GaussLogLevel {
    Fatal,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Result of a `check` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussConnectionStatus {
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
pub struct GaussControlMessage {
    #[serde(rename = "type")]
    pub control_type: GaussControlMessageType,
    /// Epoch milliseconds.
    pub emitted_at: f64,
    #[serde(rename = "connectorConfig", skip_serializing_if = "Option::is_none")]
    pub connector_config: Option<GaussControlConnectorConfigMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GaussControlMessageType {
    ConnectorConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussControlConnectorConfigMessage {
    pub config: Value,
}

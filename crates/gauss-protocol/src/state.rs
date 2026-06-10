//! STATE messages — checkpoints that make syncs resumable.
//!
//! Covers protocol "state v2": `STREAM` (per-stream), `GLOBAL` (shared +
//! per-stream, e.g. CDC), and `LEGACY` (single opaque blob).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteStateMessage {
    /// Absent means LEGACY in old connectors.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub state_type: Option<AirbyteStateType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<AirbyteStreamState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global: Option<AirbyteGlobalState>,
    /// Legacy whole-source state blob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(rename = "sourceStats", skip_serializing_if = "Option::is_none")]
    pub source_stats: Option<AirbyteStateStats>,
    #[serde(rename = "destinationStats", skip_serializing_if = "Option::is_none")]
    pub destination_stats: Option<AirbyteStateStats>,
}

impl AirbyteStateMessage {
    pub fn stream(stream_state: AirbyteStreamState) -> Self {
        Self {
            state_type: Some(AirbyteStateType::Stream),
            stream: Some(stream_state),
            global: None,
            data: None,
            source_stats: None,
            destination_stats: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AirbyteStateType {
    Global,
    Stream,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteStreamState {
    pub stream_descriptor: StreamDescriptor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_state: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AirbyteGlobalState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_state: Option<Value>,
    pub stream_states: Vec<AirbyteStreamState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamDescriptor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

impl StreamDescriptor {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            namespace: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AirbyteStateStats {
    #[serde(rename = "recordCount", skip_serializing_if = "Option::is_none")]
    pub record_count: Option<f64>,
}

//! STATE messages — checkpoints that make syncs resumable.
//!
//! Covers protocol "state v2": `STREAM` (per-stream), `GLOBAL` (shared +
//! per-stream, e.g. CDC), and `LEGACY` (single opaque blob).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussStateMessage {
    /// Absent means LEGACY in old connectors.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub state_type: Option<GaussStateType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<GaussStreamState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global: Option<GaussGlobalState>,
    /// Legacy whole-source state blob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(rename = "sourceStats", skip_serializing_if = "Option::is_none")]
    pub source_stats: Option<GaussStateStats>,
    #[serde(rename = "destinationStats", skip_serializing_if = "Option::is_none")]
    pub destination_stats: Option<GaussStateStats>,
}

impl GaussStateMessage {
    pub fn stream(stream_state: GaussStreamState) -> Self {
        Self {
            state_type: Some(GaussStateType::Stream),
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
pub enum GaussStateType {
    Global,
    Stream,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussStreamState {
    pub stream_descriptor: StreamDescriptor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_state: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussGlobalState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_state: Option<Value>,
    pub stream_states: Vec<GaussStreamState>,
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
pub struct GaussStateStats {
    #[serde(rename = "recordCount", skip_serializing_if = "Option::is_none")]
    pub record_count: Option<f64>,
}

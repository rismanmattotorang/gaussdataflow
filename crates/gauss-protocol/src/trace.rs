//! TRACE messages — errors, progress estimates, stream status, analytics.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::StreamDescriptor;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussTraceMessage {
    #[serde(rename = "type")]
    pub trace_type: GaussTraceType,
    /// Epoch milliseconds.
    pub emitted_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<GaussErrorTraceMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<GaussEstimateTraceMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_status: Option<GaussStreamStatusTraceMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analytics: Option<GaussAnalyticsTraceMessage>,
}

impl GaussTraceMessage {
    pub fn stream_status(emitted_at: f64, status: GaussStreamStatusTraceMessage) -> Self {
        Self {
            trace_type: GaussTraceType::StreamStatus,
            emitted_at,
            error: None,
            estimate: None,
            stream_status: Some(status),
            analytics: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GaussTraceType {
    Error,
    Estimate,
    StreamStatus,
    Analytics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussErrorTraceMessage {
    /// User-facing summary of the failure.
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_type: Option<FailureType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_descriptor: Option<StreamDescriptor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureType {
    SystemError,
    ConfigError,
    TransientError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussEstimateTraceMessage {
    pub name: String,
    #[serde(rename = "type")]
    pub estimate_type: EstimateType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_estimate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_estimate: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EstimateType {
    Stream,
    Sync,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussStreamStatusTraceMessage {
    pub stream_descriptor: StreamDescriptor,
    pub status: StreamStatus,
    /// Structured reasons (e.g. rate-limited); kept opaque.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasons: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StreamStatus {
    Started,
    Running,
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussAnalyticsTraceMessage {
    #[serde(rename = "type")]
    pub analytics_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

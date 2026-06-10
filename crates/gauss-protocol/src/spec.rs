//! `ConnectorSpecification` — the result of the `spec` operation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::DestinationSyncMode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectorSpecification {
    #[serde(rename = "documentationUrl", skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    #[serde(rename = "changelogUrl", skip_serializing_if = "Option::is_none")]
    pub changelog_url: Option<String>,
    /// JSON Schema for the connector's configuration object.
    #[serde(rename = "connectionSpecification")]
    pub connection_specification: Value,
    #[serde(
        rename = "supportsIncremental",
        skip_serializing_if = "Option::is_none"
    )]
    pub supports_incremental: Option<bool>,
    #[serde(
        rename = "supportsNormalization",
        skip_serializing_if = "Option::is_none"
    )]
    pub supports_normalization: Option<bool>,
    #[serde(rename = "supportsDBT", skip_serializing_if = "Option::is_none")]
    pub supports_dbt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_destination_sync_modes: Option<Vec<DestinationSyncMode>>,
    /// OAuth configuration; kept opaque until Phase 6 (OAuth flows).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advanced_auth: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
}

impl ConnectorSpecification {
    pub fn new(connection_specification: Value) -> Self {
        Self {
            documentation_url: None,
            changelog_url: None,
            connection_specification,
            supports_incremental: None,
            supports_normalization: None,
            supports_dbt: None,
            supported_destination_sync_modes: None,
            advanced_auth: None,
            protocol_version: None,
        }
    }
}

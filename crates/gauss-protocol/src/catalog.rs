//! Catalogs: what a source can emit (`GaussCatalog`, from `discover`) and
//! what the user chose to sync (`ConfiguredGaussCatalog`, input to `read`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussCatalog {
    pub streams: Vec<GaussStream>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussStream {
    pub name: String,
    /// JSON Schema describing the records of this stream.
    pub json_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_sync_modes: Option<Vec<SyncMode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_defined_cursor: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_cursor_field: Option<Vec<String>>,
    /// Each element is the path to one component of a (possibly composite)
    /// primary key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_defined_primary_key: Option<Vec<Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_resumable: Option<bool>,
}

impl GaussStream {
    pub fn new(name: impl Into<String>, json_schema: Value) -> Self {
        Self {
            name: name.into(),
            json_schema,
            supported_sync_modes: None,
            source_defined_cursor: None,
            default_cursor_field: None,
            source_defined_primary_key: None,
            namespace: None,
            is_resumable: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredGaussCatalog {
    pub streams: Vec<ConfiguredGaussStream>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredGaussStream {
    pub stream: GaussStream,
    pub sync_mode: SyncMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_field: Option<Vec<String>>,
    pub destination_sync_mode: DestinationSyncMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<Vec<Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_generation_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_id: Option<i64>,
}

impl ConfiguredGaussStream {
    /// A full-refresh/append configuration — the lowest common denominator
    /// every source supports.
    pub fn full_refresh(stream: GaussStream) -> Self {
        Self {
            stream,
            sync_mode: SyncMode::FullRefresh,
            cursor_field: None,
            destination_sync_mode: DestinationSyncMode::Append,
            primary_key: None,
            generation_id: None,
            minimum_generation_id: None,
            sync_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    FullRefresh,
    Incremental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationSyncMode {
    Append,
    Overwrite,
    AppendDedup,
    OverwriteDedup,
}

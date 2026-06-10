//! The declarative manifest model: a YAML/JSON document describing an HTTP
//! API source — base requester, authentication, streams, pagination,
//! incremental cursors. Deliberately small and explicit; it grows with real
//! connector needs, not speculatively.
//!
//! ```yaml
//! requester:
//!   url_base: https://api.example.com
//!   authenticator: { type: api_key, header: X-Api-Key, api_token: "{{ config.api_key }}" }
//! streams:
//!   - name: users
//!     path: /users
//!     record_selector: data
//!     primary_key: [id]
//!     cursor_field: updated_at
//!     paginator: { type: offset, page_size: 100 }
//! spec:
//!   connection_specification:
//!     type: object
//!     properties: { api_key: { type: string, gauss_secret: true } }
//! ```

use std::collections::BTreeMap;

use gauss_cdk::CdkError;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub requester: Requester,
    pub streams: Vec<StreamDef>,
    /// Connector spec; defaults to a permissive object schema.
    #[serde(default)]
    pub spec: Option<SpecSection>,
    /// Which stream `check` probes; defaults to the first.
    #[serde(default)]
    pub check: Option<CheckSection>,
}

#[derive(Debug, Deserialize)]
pub struct SpecSection {
    pub connection_specification: Value,
    #[serde(default)]
    pub documentation_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CheckSection {
    pub stream: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requester {
    pub url_base: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub authenticator: Option<Authenticator>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Authenticator {
    ApiKey {
        header: String,
        api_token: String,
    },
    Bearer {
        api_token: String,
    },
    Basic {
        username: String,
        #[serde(default)]
        password: String,
    },
    NoAuth {},
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamDef {
    pub name: String,
    pub path: String,
    /// Static query parameters (values support interpolation).
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Dot-path into the response JSON to the record array; absent means the
    /// response root is the array.
    #[serde(default)]
    pub record_selector: Option<String>,
    #[serde(default)]
    pub primary_key: Option<Vec<String>>,
    /// Record field used for incremental sync (compared per record;
    /// max value checkpointed).
    #[serde(default)]
    pub cursor_field: Option<String>,
    /// JSON Schema for records; defaults to a permissive object.
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default)]
    pub paginator: Option<Paginator>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Paginator {
    /// `?limit=N&offset=K`, advancing while full pages return.
    Offset {
        page_size: u64,
        #[serde(default = "default_limit_param")]
        limit_param: String,
        #[serde(default = "default_offset_param")]
        offset_param: String,
    },
    /// `?page=N`, advancing while pages are non-empty.
    Page {
        #[serde(default = "default_page_param")]
        page_param: String,
        #[serde(default)]
        page_size: Option<u64>,
        #[serde(default)]
        size_param: Option<String>,
        #[serde(default = "default_start_page")]
        start_page: u64,
    },
    /// Token from the response body (`cursor_path`) passed back as
    /// `cursor_param`, until absent/null.
    Cursor {
        cursor_path: String,
        cursor_param: String,
    },
}

fn default_limit_param() -> String {
    "limit".into()
}
fn default_offset_param() -> String {
    "offset".into()
}
fn default_page_param() -> String {
    "page".into()
}
fn default_start_page() -> u64 {
    1
}

impl Manifest {
    /// Load from the connector config's reserved `manifest` key — either an
    /// inline object or a YAML string. Keeping the manifest in the config
    /// means declarative connectors flow through the platform (registry,
    /// secret envelope, launchers) like any other connector.
    pub fn from_config(config: &Value) -> Result<Self, CdkError> {
        let raw = config.get("manifest").ok_or_else(|| {
            CdkError::Config("config must contain a `manifest` object or YAML string".into())
        })?;
        match raw {
            Value::String(yaml) => serde_yaml::from_str(yaml)
                .map_err(|e| CdkError::Config(format!("invalid manifest YAML: {e}"))),
            object => serde_json::from_value(object.clone())
                .map_err(|e| CdkError::Config(format!("invalid manifest: {e}"))),
        }
    }
}

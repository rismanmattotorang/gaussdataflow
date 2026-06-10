//! Secret envelope for connector configurations.
//!
//! Connector specs mark sensitive fields with `"gauss_secret": true` (or the
//! legacy `"airbyte_secret": true` keyword used by third-party connector
//! specs) in their JSON Schema. The platform must never persist those values alongside
//! the rest of the configuration. This crate implements the envelope:
//!
//! - [`split_config`] walks a config against its spec schema and replaces
//!   each secret value with a `{"_secret": "<id>"}` reference, returning the
//!   redacted config plus the extracted `(id, value)` pairs. The redacted
//!   form is what gets persisted and what the API returns.
//! - [`hydrate_config`] does the reverse just-in-time, resolving references
//!   through a [`SecretsBackend`] right before a connector is launched.
//!
//! Backends are pluggable (Postgres in `gauss-store` today; a vault later).

pub mod vault;
pub use vault::VaultSecretsBackend;

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use uuid::Uuid;

/// Key used for secret references inside persisted configurations.
pub const SECRET_REF_KEY: &str = "_secret";

/// Schema keywords marking a property as sensitive. `gauss_secret` is the
/// native keyword; the second is accepted for compatibility with third-party
/// connector specs.
const SECRET_SCHEMA_KEYS: [&str; 2] = ["gauss_secret", "airbyte_secret"];

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("secret `{0}` not found in backend")]
    NotFound(String),
    #[error("secrets backend error: {0}")]
    Backend(String),
}

/// Storage for raw secret values, keyed by opaque string ids.
#[async_trait::async_trait]
pub trait SecretsBackend: Send + Sync {
    async fn put(&self, id: &str, value: &str) -> Result<(), SecretsError>;
    async fn get(&self, id: &str) -> Result<String, SecretsError>;
    async fn delete(&self, id: &str) -> Result<(), SecretsError>;
}

/// In-memory backend for tests and ephemeral dev runs.
#[derive(Default)]
pub struct MemorySecretsBackend {
    values: tokio::sync::RwLock<BTreeMap<String, String>>,
}

#[async_trait::async_trait]
impl SecretsBackend for MemorySecretsBackend {
    async fn put(&self, id: &str, value: &str) -> Result<(), SecretsError> {
        self.values
            .write()
            .await
            .insert(id.to_string(), value.to_string());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<String, SecretsError> {
        self.values
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| SecretsError::NotFound(id.to_string()))
    }

    async fn delete(&self, id: &str) -> Result<(), SecretsError> {
        self.values.write().await.remove(id);
        Ok(())
    }
}

/// Replace secret values in `config` with references, guided by the spec's
/// `connectionSpecification` JSON Schema. Returns the redacted config and the
/// extracted `(id, raw_value)` pairs the caller must persist.
///
/// Values that are already references (round-tripped from an earlier split)
/// are kept as-is, so partial config updates don't re-extract or lose them.
pub fn split_config(schema: &Value, config: &Value) -> (Value, Vec<(String, String)>) {
    let mut secrets = Vec::new();
    let redacted = walk_split(schema, config, &mut secrets);
    (redacted, secrets)
}

fn walk_split(schema: &Value, value: &Value, out: &mut Vec<(String, String)>) -> Value {
    if is_secret_ref(value) {
        return value.clone();
    }

    if schema_marks_secret(schema) {
        // Secrets are scalars in practice; serialize non-strings verbatim.
        let raw = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let id = Uuid::new_v4().to_string();
        out.push((id.clone(), raw));
        return json!({ SECRET_REF_KEY: id });
    }

    match value {
        Value::Object(fields) => {
            let mut redacted = Map::with_capacity(fields.len());
            for (key, field_value) in fields {
                let field_schema = property_schema(schema, key);
                redacted.insert(key.clone(), walk_split(&field_schema, field_value, out));
            }
            Value::Object(redacted)
        }
        Value::Array(items) => {
            let item_schema = schema.get("items").cloned().unwrap_or(Value::Null);
            Value::Array(
                items
                    .iter()
                    .map(|item| walk_split(&item_schema, item, out))
                    .collect(),
            )
        }
        scalar => scalar.clone(),
    }
}

/// Resolve the subschema for `key`, merging `properties` across
/// `oneOf`/`anyOf`/`allOf` branches (condition-style specs put secrets
/// inside oneOf variants; we cannot know which branch matched, so a field
/// is secret if any branch marks it secret).
fn property_schema(schema: &Value, key: &str) -> Value {
    let mut found: Option<Value> = None;
    collect_property(schema, key, &mut found);
    found.unwrap_or(Value::Null)
}

fn collect_property(schema: &Value, key: &str, found: &mut Option<Value>) {
    if let Some(prop) = schema.get("properties").and_then(|p| p.get(key)) {
        // Prefer a branch that marks the field secret.
        if schema_marks_secret(prop) || found.is_none() {
            *found = Some(prop.clone());
        }
    }
    for combinator in ["oneOf", "anyOf", "allOf"] {
        if let Some(branches) = schema.get(combinator).and_then(Value::as_array) {
            for branch in branches {
                collect_property(branch, key, found);
            }
        }
    }
}

fn schema_marks_secret(schema: &Value) -> bool {
    SECRET_SCHEMA_KEYS
        .iter()
        .any(|key| schema.get(key).and_then(Value::as_bool) == Some(true))
}

fn is_secret_ref(value: &Value) -> bool {
    matches!(value, Value::Object(map) if map.len() == 1 && map.contains_key(SECRET_REF_KEY))
}

/// Collect every secret reference id present in a stored configuration.
pub fn collect_refs(config: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    walk_refs(config, &mut refs);
    refs
}

fn walk_refs(value: &Value, out: &mut Vec<String>) {
    if let Value::Object(map) = value {
        if map.len() == 1 {
            if let Some(Value::String(id)) = map.get(SECRET_REF_KEY) {
                out.push(id.clone());
                return;
            }
        }
        for child in map.values() {
            walk_refs(child, out);
        }
    } else if let Value::Array(items) = value {
        for item in items {
            walk_refs(item, out);
        }
    }
}

/// Replace every `{"_secret": id}` reference with the raw value from the
/// backend. Only ever call this on the path that hands the config to a
/// connector; hydrated configs must not be persisted or returned by the API.
pub async fn hydrate_config(
    config: &Value,
    backend: &dyn SecretsBackend,
) -> Result<Value, SecretsError> {
    Ok(match config {
        Value::Object(map) => {
            if map.len() == 1 {
                if let Some(Value::String(id)) = map.get(SECRET_REF_KEY) {
                    return Ok(Value::String(backend.get(id).await?));
                }
            }
            let mut hydrated = Map::with_capacity(map.len());
            for (key, child) in map {
                hydrated.insert(key.clone(), Box::pin(hydrate_config(child, backend)).await?);
            }
            Value::Object(hydrated)
        }
        Value::Array(items) => {
            let mut hydrated = Vec::with_capacity(items.len());
            for item in items {
                hydrated.push(Box::pin(hydrate_config(item, backend)).await?);
            }
            Value::Array(hydrated)
        }
        scalar => scalar.clone(),
    })
}

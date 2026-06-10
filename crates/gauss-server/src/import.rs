//! Deployment import: bootstrap a workspace — definitions, configured
//! sources/destinations (secrets sealed on the way in), and connections —
//! from one portable JSON document. Exported configs from other deployments
//! map onto this shape with a small script; `gauss-server --import-file`
//! applies it idempotently enough for re-runs (definitions upsert; actors
//! and connections are created fresh).
//!
//! ```json
//! {
//!   "workspace": "production",
//!   "sources": [{
//!     "name": "orders db",
//!     "definition": {"name": "PostgreSQL", "dockerRepository": "airbyte/source-postgres", "dockerImageTag": "latest"},
//!     "configuration": {"host": "...", "password": "raw-secret-sealed-on-import"}
//!   }],
//!   "destinations": [ ... same shape ... ],
//!   "connections": [{
//!     "name": "orders → warehouse",
//!     "source": "orders db", "destination": "warehouse",
//!     "catalog": {"streams": [...]}, "schedule": {"intervalMinutes": 60}
//!   }]
//! }
//! ```

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use gauss_secrets::SecretsBackend;
use gauss_store::{ActorType, NewActor, NewConnection, NewDefinition, Store, StoreError};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDocument {
    pub workspace: String,
    #[serde(default)]
    pub sources: Vec<ImportActor>,
    #[serde(default)]
    pub destinations: Vec<ImportActor>,
    #[serde(default)]
    pub connections: Vec<ImportConnection>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportActor {
    pub name: String,
    pub definition: ImportDefinition,
    pub configuration: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDefinition {
    pub name: String,
    pub docker_repository: String,
    pub docker_image_tag: String,
    #[serde(default)]
    pub spec: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportConnection {
    pub name: String,
    /// Source/destination referenced by their `name` in this document.
    pub source: String,
    pub destination: String,
    pub catalog: Value,
    #[serde(default)]
    pub schedule: Option<Value>,
    #[serde(default)]
    pub notifications: Option<Value>,
}

#[derive(Debug, Default)]
pub struct ImportSummary {
    pub sources: usize,
    pub destinations: usize,
    pub connections: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("secrets error: {0}")]
    Secrets(#[from] gauss_secrets::SecretsError),
    #[error("{0}")]
    Invalid(String),
}

pub async fn import(
    store: &Store,
    secrets: &dyn SecretsBackend,
    doc: ImportDocument,
) -> Result<ImportSummary, ImportError> {
    let workspace = store.workspaces().create(&doc.workspace).await?;
    let mut summary = ImportSummary::default();
    let mut actor_ids: HashMap<(ActorType, String), Uuid> = HashMap::new();

    for (actor_type, actors) in [
        (ActorType::Source, &doc.sources),
        (ActorType::Destination, &doc.destinations),
    ] {
        for actor in actors {
            let definition = store
                .definitions()
                .upsert(&NewDefinition {
                    id: Uuid::new_v4(),
                    actor_type,
                    name: actor.definition.name.clone(),
                    docker_repository: actor.definition.docker_repository.clone(),
                    docker_image_tag: actor.definition.docker_image_tag.clone(),
                    documentation_url: None,
                    spec: actor.definition.spec.clone(),
                })
                .await?;

            // Seal secrets exactly like the live API would.
            let schema = definition
                .spec
                .as_ref()
                .and_then(|s| s.0.get("connectionSpecification").cloned())
                .unwrap_or_else(|| json!({}));
            let (redacted, sealed) = gauss_secrets::split_config(&schema, &actor.configuration);
            for (id, value) in &sealed {
                secrets.put(id, value).await?;
            }

            let created = store
                .actors()
                .create(&NewActor {
                    workspace_id: workspace.id,
                    definition_id: definition.id,
                    actor_type,
                    name: actor.name.clone(),
                    configuration: redacted,
                })
                .await?;
            actor_ids.insert((actor_type, actor.name.clone()), created.id);
            match actor_type {
                ActorType::Source => summary.sources += 1,
                ActorType::Destination => summary.destinations += 1,
            }
        }
    }

    for connection in &doc.connections {
        let source_id = actor_ids
            .get(&(ActorType::Source, connection.source.clone()))
            .ok_or_else(|| {
                ImportError::Invalid(format!(
                    "connection `{}` references unknown source `{}`",
                    connection.name, connection.source
                ))
            })?;
        let destination_id = actor_ids
            .get(&(ActorType::Destination, connection.destination.clone()))
            .ok_or_else(|| {
                ImportError::Invalid(format!(
                    "connection `{}` references unknown destination `{}`",
                    connection.name, connection.destination
                ))
            })?;
        store
            .connections()
            .create(&NewConnection {
                workspace_id: workspace.id,
                source_id: *source_id,
                destination_id: *destination_id,
                name: connection.name.clone(),
                catalog: connection.catalog.clone(),
                schedule: connection.schedule.clone(),
                notifications: connection.notifications.clone(),
            })
            .await?;
        summary.connections += 1;
    }

    Ok(summary)
}

//! Connector-registry ingestion.
//!
//! Accepts a registry document shaped like Gauss's public connector
//! registry JSON (`{"sources": [...], "destinations": [...]}`); entries are
//! parsed tolerantly so both the upstream registry format and hand-written
//! seed files import cleanly. Imports are idempotent upserts keyed on
//! `(actor_type, dockerRepository)`.

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use gauss_store::{ActorType, NewDefinition, Store, StoreError};

#[derive(Deserialize)]
pub struct RegistryDocument {
    #[serde(default)]
    pub sources: Vec<RegistryEntry>,
    #[serde(default)]
    pub destinations: Vec<RegistryEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    // The upstream registry uses type-specific id keys; seed files may use
    // a plain `definitionId`. Absent ids get a fresh UUID on first import.
    pub source_definition_id: Option<Uuid>,
    pub destination_definition_id: Option<Uuid>,
    pub definition_id: Option<Uuid>,
    pub name: String,
    pub docker_repository: String,
    pub docker_image_tag: String,
    pub documentation_url: Option<String>,
    pub spec: Option<Value>,
}

impl RegistryEntry {
    fn into_definition(self, actor_type: ActorType) -> NewDefinition {
        NewDefinition {
            id: self
                .source_definition_id
                .or(self.destination_definition_id)
                .or(self.definition_id)
                .unwrap_or_else(Uuid::new_v4),
            actor_type,
            name: self.name,
            docker_repository: self.docker_repository,
            docker_image_tag: self.docker_image_tag,
            documentation_url: self.documentation_url,
            spec: self.spec,
        }
    }
}

pub struct ImportSummary {
    pub sources: usize,
    pub destinations: usize,
}

pub async fn import(store: &Store, doc: RegistryDocument) -> Result<ImportSummary, StoreError> {
    let mut summary = ImportSummary {
        sources: 0,
        destinations: 0,
    };
    for entry in doc.sources {
        store
            .definitions()
            .upsert(&entry.into_definition(ActorType::Source))
            .await?;
        summary.sources += 1;
    }
    for entry in doc.destinations {
        store
            .definitions()
            .upsert(&entry.into_definition(ActorType::Destination))
            .await?;
        summary.destinations += 1;
    }
    Ok(summary)
}

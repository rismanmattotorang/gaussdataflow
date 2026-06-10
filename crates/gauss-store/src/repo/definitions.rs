use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::{ActorDefinition, ActorType};

pub struct NewDefinition {
    pub id: Uuid,
    pub actor_type: ActorType,
    pub name: String,
    pub docker_repository: String,
    pub docker_image_tag: String,
    pub documentation_url: Option<String>,
    pub spec: Option<Value>,
}

pub struct DefinitionRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

const COLUMNS: &str = "id, actor_type, name, docker_repository, docker_image_tag, \
                       documentation_url, spec, created_at, updated_at";

impl DefinitionRepo<'_> {
    /// Insert or refresh a registry entry. Conflicts on
    /// `(actor_type, docker_repository)` update the existing row, so registry
    /// re-imports are idempotent and pick up new tags/specs.
    pub async fn upsert(&self, def: &NewDefinition) -> Result<ActorDefinition, StoreError> {
        sqlx::query_as::<_, ActorDefinition>(&format!(
            "INSERT INTO actor_definitions
                 (id, actor_type, name, docker_repository, docker_image_tag,
                  documentation_url, spec)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (actor_type, docker_repository) DO UPDATE SET
                 name = EXCLUDED.name,
                 docker_image_tag = EXCLUDED.docker_image_tag,
                 documentation_url = EXCLUDED.documentation_url,
                 spec = COALESCE(EXCLUDED.spec, actor_definitions.spec),
                 updated_at = now()
             RETURNING {COLUMNS}"
        ))
        .bind(def.id)
        .bind(def.actor_type)
        .bind(&def.name)
        .bind(&def.docker_repository)
        .bind(&def.docker_image_tag)
        .bind(&def.documentation_url)
        .bind(def.spec.clone().map(Json))
        .fetch_one(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "definition"))
    }

    pub async fn list(&self, actor_type: ActorType) -> Result<Vec<ActorDefinition>, StoreError> {
        Ok(sqlx::query_as::<_, ActorDefinition>(&format!(
            "SELECT {COLUMNS} FROM actor_definitions WHERE actor_type = $1 ORDER BY name"
        ))
        .bind(actor_type)
        .fetch_all(self.pool)
        .await?)
    }

    pub async fn get(&self, id: Uuid) -> Result<ActorDefinition, StoreError> {
        sqlx::query_as::<_, ActorDefinition>(&format!(
            "SELECT {COLUMNS} FROM actor_definitions WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::NotFound("definition"))
    }
}

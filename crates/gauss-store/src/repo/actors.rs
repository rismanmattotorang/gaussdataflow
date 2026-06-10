use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::{Actor, ActorType};

pub struct NewActor {
    pub workspace_id: Uuid,
    pub definition_id: Uuid,
    pub actor_type: ActorType,
    pub name: String,
    /// Redacted configuration (secrets already extracted by the caller).
    pub configuration: Value,
}

pub struct ActorRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

const COLUMNS: &str = "id, workspace_id, definition_id, actor_type, name, configuration, \
                       created_at, updated_at";

impl ActorRepo<'_> {
    pub async fn create(&self, actor: &NewActor) -> Result<Actor, StoreError> {
        sqlx::query_as::<_, Actor>(&format!(
            "INSERT INTO actors
                 (workspace_id, definition_id, actor_type, name, configuration)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING {COLUMNS}"
        ))
        .bind(actor.workspace_id)
        .bind(actor.definition_id)
        .bind(actor.actor_type)
        .bind(&actor.name)
        .bind(Json(actor.configuration.clone()))
        .fetch_one(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "actor"))
    }

    pub async fn list(
        &self,
        workspace_id: Uuid,
        actor_type: ActorType,
    ) -> Result<Vec<Actor>, StoreError> {
        Ok(sqlx::query_as::<_, Actor>(&format!(
            "SELECT {COLUMNS} FROM actors
             WHERE workspace_id = $1 AND actor_type = $2 ORDER BY created_at"
        ))
        .bind(workspace_id)
        .bind(actor_type)
        .fetch_all(self.pool)
        .await?)
    }

    pub async fn get(&self, id: Uuid, actor_type: ActorType) -> Result<Actor, StoreError> {
        sqlx::query_as::<_, Actor>(&format!(
            "SELECT {COLUMNS} FROM actors WHERE id = $1 AND actor_type = $2"
        ))
        .bind(id)
        .bind(actor_type)
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::NotFound("actor"))
    }

    pub async fn update(
        &self,
        id: Uuid,
        actor_type: ActorType,
        name: Option<&str>,
        configuration: Option<&Value>,
    ) -> Result<Actor, StoreError> {
        sqlx::query_as::<_, Actor>(&format!(
            "UPDATE actors SET
                 name = COALESCE($3, name),
                 configuration = COALESCE($4, configuration),
                 updated_at = now()
             WHERE id = $1 AND actor_type = $2
             RETURNING {COLUMNS}"
        ))
        .bind(id)
        .bind(actor_type)
        .bind(name)
        .bind(configuration.map(|c| Json(c.clone())))
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::NotFound("actor"))
    }

    pub async fn delete(&self, id: Uuid, actor_type: ActorType) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM actors WHERE id = $1 AND actor_type = $2")
            .bind(id)
            .bind(actor_type)
            .execute(self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("actor"));
        }
        Ok(())
    }
}

use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StoreError;

pub struct ConnectionStateRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

impl ConnectionStateRepo<'_> {
    pub async fn get(&self, connection_id: Uuid) -> Result<Option<Value>, StoreError> {
        let row: Option<(Json<Value>,)> =
            sqlx::query_as("SELECT state FROM connection_states WHERE connection_id = $1")
                .bind(connection_id)
                .fetch_optional(self.pool)
                .await?;
        Ok(row.map(|(state,)| state.0))
    }

    pub async fn set(&self, connection_id: Uuid, state: &Value) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO connection_states (connection_id, state) VALUES ($1, $2)
             ON CONFLICT (connection_id)
             DO UPDATE SET state = EXCLUDED.state, updated_at = now()",
        )
        .bind(connection_id)
        .bind(Json(state.clone()))
        .execute(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "connection state"))?;
        Ok(())
    }

    pub async fn clear(&self, connection_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM connection_states WHERE connection_id = $1")
            .bind(connection_id)
            .execute(self.pool)
            .await?;
        Ok(())
    }
}

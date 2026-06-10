use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::Workspace;

pub struct WorkspaceRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

impl WorkspaceRepo<'_> {
    pub async fn create(&self, name: &str) -> Result<Workspace, StoreError> {
        sqlx::query_as::<_, Workspace>(
            "INSERT INTO workspaces (name) VALUES ($1)
             RETURNING id, name, created_at, updated_at",
        )
        .bind(name)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "workspace"))
    }

    pub async fn list(&self) -> Result<Vec<Workspace>, StoreError> {
        Ok(sqlx::query_as::<_, Workspace>(
            "SELECT id, name, created_at, updated_at FROM workspaces ORDER BY created_at",
        )
        .fetch_all(self.pool)
        .await?)
    }

    pub async fn get(&self, id: Uuid) -> Result<Workspace, StoreError> {
        sqlx::query_as::<_, Workspace>(
            "SELECT id, name, created_at, updated_at FROM workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::NotFound("workspace"))
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM workspaces WHERE id = $1")
            .bind(id)
            .execute(self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("workspace"));
        }
        Ok(())
    }
}

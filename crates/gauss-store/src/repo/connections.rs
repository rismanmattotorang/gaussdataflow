use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::{Connection, ConnectionStatus};

pub struct NewConnection {
    pub workspace_id: Uuid,
    pub source_id: Uuid,
    pub destination_id: Uuid,
    pub name: String,
    /// ConfiguredAirbyteCatalog wire form.
    pub catalog: Value,
    pub schedule: Option<Value>,
}

#[derive(Default)]
pub struct ConnectionPatch {
    pub name: Option<String>,
    pub status: Option<ConnectionStatus>,
    pub catalog: Option<Value>,
    pub schedule: Option<Value>,
}

pub struct ConnectionRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

#[derive(sqlx::FromRow)]
struct SchedulableRow {
    #[sqlx(flatten)]
    connection: Connection,
    last_job_at: Option<chrono::DateTime<chrono::Utc>>,
}

const COLUMNS: &str = "id, workspace_id, source_id, destination_id, name, status, catalog, \
                       schedule, created_at, updated_at";

impl ConnectionRepo<'_> {
    pub async fn create(&self, conn: &NewConnection) -> Result<Connection, StoreError> {
        sqlx::query_as::<_, Connection>(&format!(
            "INSERT INTO connections
                 (workspace_id, source_id, destination_id, name, catalog, schedule)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING {COLUMNS}"
        ))
        .bind(conn.workspace_id)
        .bind(conn.source_id)
        .bind(conn.destination_id)
        .bind(&conn.name)
        .bind(Json(conn.catalog.clone()))
        .bind(conn.schedule.clone().map(Json))
        .fetch_one(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "connection"))
    }

    pub async fn list(&self, workspace_id: Uuid) -> Result<Vec<Connection>, StoreError> {
        Ok(sqlx::query_as::<_, Connection>(&format!(
            "SELECT {COLUMNS} FROM connections WHERE workspace_id = $1 ORDER BY created_at"
        ))
        .bind(workspace_id)
        .fetch_all(self.pool)
        .await?)
    }

    pub async fn get(&self, id: Uuid) -> Result<Connection, StoreError> {
        sqlx::query_as::<_, Connection>(&format!("SELECT {COLUMNS} FROM connections WHERE id = $1"))
            .bind(id)
            .fetch_optional(self.pool)
            .await?
            .ok_or(StoreError::NotFound("connection"))
    }

    pub async fn update(
        &self,
        id: Uuid,
        patch: &ConnectionPatch,
    ) -> Result<Connection, StoreError> {
        sqlx::query_as::<_, Connection>(&format!(
            "UPDATE connections SET
                 name = COALESCE($2, name),
                 status = COALESCE($3, status),
                 catalog = COALESCE($4, catalog),
                 schedule = COALESCE($5, schedule),
                 updated_at = now()
             WHERE id = $1
             RETURNING {COLUMNS}"
        ))
        .bind(id)
        .bind(&patch.name)
        .bind(patch.status.map(|s| s.as_str()))
        .bind(patch.catalog.clone().map(Json))
        .bind(patch.schedule.clone().map(Json))
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::NotFound("connection"))
    }

    /// Scheduler input: active connections that have a schedule and no
    /// pending/running job, with the creation time of their most recent job.
    pub async fn list_schedulable(
        &self,
    ) -> Result<Vec<(Connection, Option<chrono::DateTime<chrono::Utc>>)>, StoreError> {
        let rows: Vec<SchedulableRow> = sqlx::query_as(&format!(
            "SELECT {COLUMNS},
                    (SELECT MAX(j.created_at) FROM jobs j
                     WHERE j.connection_id = connections.id) AS last_job_at
             FROM connections
             WHERE status = 'active'
               AND schedule IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM jobs j
                   WHERE j.connection_id = connections.id
                     AND j.status IN ('pending', 'running')
               )"
        ))
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.connection, row.last_job_at))
            .collect())
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM connections WHERE id = $1")
            .bind(id)
            .execute(self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("connection"));
        }
        Ok(())
    }
}

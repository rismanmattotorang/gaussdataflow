//! Postgres-backed [`SecretsBackend`] — the local/dev default.
//!
//! Values are stored as-is in a dedicated table, referenced from actor
//! configurations by id only. Production deployments should swap in an
//! external secret manager behind the same trait (Phase 6).

use gauss_secrets::{SecretsBackend, SecretsError};
use sqlx::PgPool;

pub struct PgSecretsBackend {
    pool: PgPool,
}

impl PgSecretsBackend {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn backend_err(err: sqlx::Error) -> SecretsError {
    SecretsError::Backend(err.to_string())
}

#[async_trait::async_trait]
impl SecretsBackend for PgSecretsBackend {
    async fn put(&self, id: &str, value: &str) -> Result<(), SecretsError> {
        sqlx::query(
            "INSERT INTO secrets (id, value) VALUES ($1, $2)
                     ON CONFLICT (id) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(id)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<String, SecretsError> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM secrets WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend_err)?;
        row.map(|(value,)| value)
            .ok_or_else(|| SecretsError::NotFound(id.to_string()))
    }

    async fn delete(&self, id: &str) -> Result<(), SecretsError> {
        sqlx::query("DELETE FROM secrets WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;
        Ok(())
    }
}

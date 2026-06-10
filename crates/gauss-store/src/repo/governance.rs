use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StoreError;

/// An API token's metadata — the hash never leaves the database, the raw
/// token never enters it.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiToken {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub subject: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub created_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, name, role, created_at, last_used_at";

pub struct TokenRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

impl TokenRepo<'_> {
    pub async fn create(
        &self,
        name: &str,
        role: &str,
        token_hash: &str,
    ) -> Result<ApiToken, StoreError> {
        sqlx::query_as::<_, ApiToken>(&format!(
            "INSERT INTO api_tokens (name, role, token_hash) VALUES ($1, $2, $3)
             RETURNING {COLUMNS}"
        ))
        .bind(name)
        .bind(role)
        .bind(token_hash)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "api token"))
    }

    /// Look up by token hash and touch `last_used_at`.
    pub async fn authenticate(&self, token_hash: &str) -> Result<Option<ApiToken>, StoreError> {
        Ok(sqlx::query_as::<_, ApiToken>(&format!(
            "UPDATE api_tokens SET last_used_at = now() WHERE token_hash = $1
             RETURNING {COLUMNS}"
        ))
        .bind(token_hash)
        .fetch_optional(self.pool)
        .await?)
    }

    pub async fn list(&self) -> Result<Vec<ApiToken>, StoreError> {
        Ok(sqlx::query_as::<_, ApiToken>(&format!(
            "SELECT {COLUMNS} FROM api_tokens ORDER BY created_at"
        ))
        .fetch_all(self.pool)
        .await?)
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM api_tokens WHERE id = $1")
            .bind(id)
            .execute(self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("api token"));
        }
        Ok(())
    }
}

pub struct AuditRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

impl AuditRepo<'_> {
    pub async fn record(
        &self,
        subject: &str,
        method: &str,
        path: &str,
        status: i32,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO audit_log (subject, method, path, status) VALUES ($1, $2, $3, $4)",
        )
        .bind(subject)
        .bind(method)
        .bind(path)
        .bind(status)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(&self, limit: i64) -> Result<Vec<AuditEntry>, StoreError> {
        Ok(sqlx::query_as::<_, AuditEntry>(
            "SELECT id, subject, method, path, status, created_at FROM audit_log
             ORDER BY created_at DESC, id DESC LIMIT $1",
        )
        .bind(limit.clamp(1, 1000))
        .fetch_all(self.pool)
        .await?)
    }
}

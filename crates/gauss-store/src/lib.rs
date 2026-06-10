//! Postgres persistence for gaussdataflow.
//!
//! [`Store`] wraps a connection pool and exposes typed repositories. All
//! queries are runtime-checked (`sqlx::query_as` with bind parameters) so the
//! crate builds without a live database; integration tests exercise them
//! against real Postgres.

mod error;
mod models;
mod repo;
mod secrets_backend;

pub use error::StoreError;
pub use models::*;
pub use repo::connections::{ConnectionPatch, NewConnection};
pub use repo::governance::{ApiToken, AuditEntry};
pub use repo::{actors::NewActor, definitions::NewDefinition};
pub use secrets_backend::PgSecretsBackend;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    /// Connect and run pending migrations.
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(database_url)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn workspaces(&self) -> repo::workspaces::WorkspaceRepo<'_> {
        repo::workspaces::WorkspaceRepo { pool: &self.pool }
    }

    pub fn definitions(&self) -> repo::definitions::DefinitionRepo<'_> {
        repo::definitions::DefinitionRepo { pool: &self.pool }
    }

    pub fn actors(&self) -> repo::actors::ActorRepo<'_> {
        repo::actors::ActorRepo { pool: &self.pool }
    }

    pub fn connections(&self) -> repo::connections::ConnectionRepo<'_> {
        repo::connections::ConnectionRepo { pool: &self.pool }
    }

    pub fn jobs(&self) -> repo::jobs::JobRepo<'_> {
        repo::jobs::JobRepo { pool: &self.pool }
    }

    pub fn connection_states(&self) -> repo::states::ConnectionStateRepo<'_> {
        repo::states::ConnectionStateRepo { pool: &self.pool }
    }

    pub fn tokens(&self) -> repo::governance::TokenRepo<'_> {
        repo::governance::TokenRepo { pool: &self.pool }
    }

    pub fn audit(&self) -> repo::governance::AuditRepo<'_> {
        repo::governance::AuditRepo { pool: &self.pool }
    }

    /// Secrets backend persisting into this store's `secrets` table.
    pub fn secrets_backend(&self) -> PgSecretsBackend {
        PgSecretsBackend::new(self.pool.clone())
    }
}

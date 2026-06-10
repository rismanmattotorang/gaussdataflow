use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "actor_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ActorType {
    Source,
    Destination,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    #[serde(rename = "workspaceId")]
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A connector registry entry.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ActorDefinition {
    #[serde(rename = "definitionId")]
    pub id: Uuid,
    pub actor_type: ActorType,
    pub name: String,
    pub docker_repository: String,
    pub docker_image_tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    /// ConnectorSpecification wire form, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<Json<Value>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A configured source or destination. `configuration` is always the
/// redacted form (secret values replaced by `{"_secret": id}` references).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub definition_id: Uuid,
    pub actor_type: ActorType,
    pub name: String,
    pub configuration: Json<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Active,
    Inactive,
    Deprecated,
}

impl ConnectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Deprecated => "deprecated",
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: i64,
    pub connection_id: Uuid,
    pub job_type: String,
    pub status: String,
    pub scheduled_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub cancel_requested: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Job {
    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "succeeded" | "failed" | "cancelled")
    }
}

/// A job enriched with its connection's name and workspace, plus the record
/// count of its latest attempt — the shape dashboards and the TUI consume.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct JobOverview {
    pub id: i64,
    pub connection_id: Uuid,
    pub connection_name: String,
    pub workspace_id: Uuid,
    pub job_type: String,
    pub status: String,
    pub scheduled_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub cancel_requested: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub records_synced: Option<i64>,
}

/// Aggregate platform health, optionally scoped to one workspace.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStats {
    pub workspaces: i64,
    pub sources: i64,
    pub destinations: i64,
    pub connections: i64,
    pub jobs_total: i64,
    pub jobs_pending: i64,
    pub jobs_running: i64,
    pub jobs_succeeded_24h: i64,
    pub jobs_failed_24h: i64,
    pub records_synced_24h: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub id: i64,
    pub job_id: i64,
    pub attempt_number: i32,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub records_synced: Option<i64>,
    /// Final committed state of this attempt (array of state messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Json<Value>>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    pub last_heartbeat_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    #[serde(rename = "connectionId")]
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub source_id: Uuid,
    pub destination_id: Uuid,
    pub name: String,
    pub status: String,
    /// ConfiguredGaussCatalog wire form.
    pub catalog: Json<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Json<Value>>,
    /// e.g. `{"webhookUrl": "https://..."}` — posted on job completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifications: Option<Json<Value>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

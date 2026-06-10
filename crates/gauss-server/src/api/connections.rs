use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use gauss_protocol::ConfiguredAirbyteCatalog;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::actors::WorkspaceFilter;
use super::ApiError;
use crate::AppState;
use gauss_store::{ActorType, Connection, ConnectionPatch, ConnectionStatus, NewConnection};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnection {
    pub name: String,
    pub source_id: Uuid,
    pub destination_id: Uuid,
    /// ConfiguredAirbyteCatalog wire form.
    pub catalog: Value,
    pub schedule: Option<Value>,
    pub notifications: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnection {
    pub name: Option<String>,
    pub status: Option<ConnectionStatus>,
    pub catalog: Option<Value>,
    pub schedule: Option<Value>,
    pub notifications: Option<Value>,
}

fn validate_catalog(catalog: &Value) -> Result<(), ApiError> {
    serde_json::from_value::<ConfiguredAirbyteCatalog>(catalog.clone())
        .map_err(|err| ApiError::bad_request(format!("invalid configured catalog: {err}")))?;
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateConnection>,
) -> Result<(StatusCode, Json<Connection>), ApiError> {
    validate_catalog(&body.catalog)?;

    let source = state
        .store
        .actors()
        .get(body.source_id, ActorType::Source)
        .await?;
    let destination = state
        .store
        .actors()
        .get(body.destination_id, ActorType::Destination)
        .await?;
    if source.workspace_id != destination.workspace_id {
        return Err(ApiError::bad_request(
            "source and destination belong to different workspaces",
        ));
    }

    let connection = state
        .store
        .connections()
        .create(&NewConnection {
            workspace_id: source.workspace_id,
            source_id: body.source_id,
            destination_id: body.destination_id,
            name: body.name,
            catalog: body.catalog,
            schedule: body.schedule,
            notifications: body.notifications,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(connection)))
}

pub async fn list(
    State(state): State<AppState>,
    Query(filter): Query<WorkspaceFilter>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(super::data(
        state.store.connections().list(filter.workspace_id).await?,
    ))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Connection>, ApiError> {
    Ok(Json(state.store.connections().get(id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateConnection>,
) -> Result<Json<Connection>, ApiError> {
    if let Some(catalog) = &body.catalog {
        validate_catalog(catalog)?;
    }
    let connection = state
        .store
        .connections()
        .update(
            id,
            &ConnectionPatch {
                name: body.name,
                status: body.status,
                catalog: body.catalog,
                schedule: body.schedule,
                notifications: body.notifications,
            },
        )
        .await?;
    Ok(Json(connection))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.store.connections().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

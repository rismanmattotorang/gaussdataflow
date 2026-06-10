use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use super::ApiError;
use crate::registry::{self, RegistryDocument};
use crate::AppState;
use gauss_store::{ActorDefinition, ActorType};

pub async fn import(
    State(state): State<AppState>,
    Json(doc): Json<RegistryDocument>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let summary = registry::import(&state.store, doc).await?;
    Ok(Json(json!({
        "sourcesImported": summary.sources,
        "destinationsImported": summary.destinations,
    })))
}

pub async fn list_sources(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(super::data(
        state.store.definitions().list(ActorType::Source).await?,
    ))
}

pub async fn list_destinations(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(super::data(
        state
            .store
            .definitions()
            .list(ActorType::Destination)
            .await?,
    ))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ActorDefinition>, ApiError> {
    Ok(Json(state.store.definitions().get(id).await?))
}

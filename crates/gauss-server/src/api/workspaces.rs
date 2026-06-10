use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::ApiError;
use crate::AppState;
use gauss_store::Workspace;

#[derive(Deserialize)]
pub struct CreateWorkspace {
    pub name: String,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateWorkspace>,
) -> Result<(StatusCode, Json<Workspace>), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("workspace name must not be empty"));
    }
    let workspace = state.store.workspaces().create(body.name.trim()).await?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

pub async fn list(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(super::data(state.store.workspaces().list().await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Workspace>, ApiError> {
    Ok(Json(state.store.workspaces().get(id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.store.workspaces().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

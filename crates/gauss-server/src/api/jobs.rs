use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiError;
use crate::AppState;
use gauss_store::Job;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentJobsQuery {
    pub workspace_id: Option<Uuid>,
    pub limit: Option<i64>,
}

/// Recent jobs across all connections (optionally one workspace), enriched
/// with connection names and record counts — the dashboard/TUI feed.
pub async fn list_recent(
    State(state): State<AppState>,
    Query(q): Query<RecentJobsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(super::data(
        state
            .store
            .jobs()
            .list_recent(q.workspace_id, q.limit.unwrap_or(50))
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsQuery {
    pub workspace_id: Option<Uuid>,
}

/// Aggregate platform health: actor/connection counts, queue depth, 24h
/// success/failure, records moved, last successful sync.
pub async fn platform_stats(
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<gauss_store::PlatformStats>, ApiError> {
    Ok(Json(
        state.store.jobs().platform_stats(q.workspace_id).await?,
    ))
}

/// Manual sync trigger. 409 when a job is already pending/running for the
/// connection; 400 when the connection isn't active.
pub async fn trigger_sync(
    State(state): State<AppState>,
    Path(connection_id): Path<Uuid>,
) -> Result<(StatusCode, Json<Job>), ApiError> {
    let connection = state.store.connections().get(connection_id).await?;
    if connection.status != "active" {
        return Err(ApiError::bad_request(format!(
            "connection is {}, not active",
            connection.status
        )));
    }
    let job = state.store.jobs().create(connection_id, "sync").await?;
    Ok((StatusCode::CREATED, Json(job)))
}

pub async fn list_for_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.store.connections().get(connection_id).await?;
    Ok(super::data(state.store.jobs().list(connection_id).await?))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let job = state.store.jobs().get(id).await?;
    let attempts = state.store.jobs().list_attempts(id).await?;
    let mut body = serde_json::to_value(&job)?;
    body["attempts"] = serde_json::to_value(&attempts)?;
    Ok(Json(body))
}

pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Job>, ApiError> {
    state.store.jobs().get(id).await?; // 404 before 409
    Ok(Json(state.store.jobs().cancel(id).await?))
}

pub async fn connection_state(
    State(state): State<AppState>,
    Path(connection_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.store.connections().get(connection_id).await?;
    let value = state.store.connection_states().get(connection_id).await?;
    Ok(Json(json!({ "state": value })))
}

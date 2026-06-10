//! Token management and audit inspection (admin-only, enforced by the auth
//! layer).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::ApiError;
use crate::auth::{generate_token, hash_token, Role};
use crate::AppState;

#[derive(Deserialize)]
pub struct CreateToken {
    pub name: String,
    pub role: String,
}

/// Create a token. The raw value appears in this response only.
pub async fn create_token(
    State(state): State<AppState>,
    Json(body): Json<CreateToken>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    if Role::parse(&body.role).is_none() {
        return Err(ApiError::bad_request(
            "role must be admin, editor, or viewer",
        ));
    }
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("token name must not be empty"));
    }
    let raw = generate_token();
    let token = state
        .store
        .tokens()
        .create(body.name.trim(), &body.role, &hash_token(&raw))
        .await?;
    let mut response = serde_json::to_value(&token)?;
    response["token"] = json!(raw);
    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn list_tokens(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(super::data(state.store.tokens().list().await?))
}

pub async fn delete_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.store.tokens().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

pub async fn list_audit(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(super::data(state.store.audit().list(query.limit).await?))
}

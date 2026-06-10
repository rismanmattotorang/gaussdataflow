pub mod actors;
pub mod connections;
pub mod definitions;
mod error;
pub mod governance;
pub mod jobs;
pub mod oauth;
pub mod workspaces;

pub use error::ApiError;

use axum::Json;
use serde_json::json;

pub async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok", "name": "gaussdataflow", "version": env!("CARGO_PKG_VERSION")}))
}

/// List envelope used by all collection endpoints.
pub(crate) fn data<T: serde::Serialize>(items: Vec<T>) -> Json<serde_json::Value> {
    Json(json!({ "data": items }))
}

//! gaussdataflow config API.
//!
//! Shape-compatible with Airbyte's public config API where practical
//! (camelCase JSON, `/api/v1/sources`-style resources) so existing tooling
//! can be adapted cheaply. Secrets never leave the server: configurations
//! are returned in redacted form with `{"_secret": id}` references.

pub mod api;
pub mod registry;
mod state;

pub use state::AppState;

use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(api::health))
        .route(
            "/api/v1/workspaces",
            post(api::workspaces::create).get(api::workspaces::list),
        )
        .route(
            "/api/v1/workspaces/{id}",
            get(api::workspaces::get_one).delete(api::workspaces::delete),
        )
        .route("/api/v1/definitions/import", post(api::definitions::import))
        .route("/api/v1/definitions/{id}", get(api::definitions::get_one))
        .route(
            "/api/v1/source_definitions",
            get(api::definitions::list_sources),
        )
        .route(
            "/api/v1/destination_definitions",
            get(api::definitions::list_destinations),
        )
        .route(
            "/api/v1/sources",
            post(api::actors::create_source).get(api::actors::list_sources),
        )
        .route(
            "/api/v1/sources/{id}",
            get(api::actors::get_source)
                .patch(api::actors::update_source)
                .delete(api::actors::delete_source),
        )
        .route(
            "/api/v1/sources/{id}/check",
            post(api::actors::check_source),
        )
        .route(
            "/api/v1/destinations",
            post(api::actors::create_destination).get(api::actors::list_destinations),
        )
        .route(
            "/api/v1/destinations/{id}",
            get(api::actors::get_destination)
                .patch(api::actors::update_destination)
                .delete(api::actors::delete_destination),
        )
        .route(
            "/api/v1/destinations/{id}/check",
            post(api::actors::check_destination),
        )
        .route(
            "/api/v1/connections",
            post(api::connections::create).get(api::connections::list),
        )
        .route(
            "/api/v1/connections/{id}",
            get(api::connections::get_one)
                .patch(api::connections::update)
                .delete(api::connections::delete),
        )
        .route(
            "/api/v1/connections/{id}/sync",
            post(api::jobs::trigger_sync),
        )
        .route(
            "/api/v1/connections/{id}/jobs",
            get(api::jobs::list_for_connection),
        )
        .route(
            "/api/v1/connections/{id}/state",
            get(api::jobs::connection_state),
        )
        .route("/api/v1/jobs/{id}", get(api::jobs::get_one))
        .route("/api/v1/jobs/{id}/cancel", post(api::jobs::cancel))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

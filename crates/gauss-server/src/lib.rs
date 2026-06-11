//! gaussdataflow config API.
//!
//! Shape-compatible with Gauss's public config API where practical
//! (camelCase JSON, `/api/v1/sources`-style resources) so existing tooling
//! can be adapted cheaply. Secrets never leave the server: configurations
//! are returned in redacted form with `{"_secret": id}` references.

pub mod api;
pub mod auth;
pub mod import;
pub mod registry;
mod state;

pub use state::AppState;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
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
            "/api/v1/sources/{id}/discover",
            post(api::actors::discover_source),
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
        .route("/api/v1/jobs", get(api::jobs::list_recent))
        .route("/api/v1/stats", get(api::jobs::platform_stats))
        .route("/api/v1/jobs/{id}", get(api::jobs::get_one))
        .route("/api/v1/jobs/{id}/cancel", post(api::jobs::cancel))
        .route(
            "/api/v1/tokens",
            post(api::governance::create_token).get(api::governance::list_tokens),
        )
        .route(
            "/api/v1/tokens/{id}",
            axum::routing::delete(api::governance::delete_token),
        )
        .route("/api/v1/audit", get(api::governance::list_audit))
        .route(
            "/api/v1/oauth/authorize_url",
            post(api::oauth::authorize_url),
        )
        .route("/api/v1/oauth/complete", post(api::oauth::complete))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::layer,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer(&state))
        .with_state(state)
}

/// The web console runs on its own origin, so CORS is permissive by default
/// for self-hosted setups; production deployments pin the console origin(s)
/// with `--cors-origin` and the layer then only allows the headers the
/// console actually sends.
fn cors_layer(state: &AppState) -> CorsLayer {
    if state.cors_origins.is_empty() {
        CorsLayer::permissive()
    } else {
        CorsLayer::new()
            .allow_origin(state.cors_origins.clone())
            .allow_methods(tower_http::cors::Any)
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
            ])
    }
}

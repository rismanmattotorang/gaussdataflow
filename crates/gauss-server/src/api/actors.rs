//! Sources and destinations ("actors") — shared handlers parameterized by
//! [`ActorType`].
//!
//! Configuration lifecycle: on create/update the config is split against the
//! definition's spec (`gauss-secrets`); raw secret values go to the secrets
//! backend and only the redacted form is persisted or returned. `check`
//! hydrates just-in-time and launches the connector via the Phase-1 runtime.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use gauss_connector_runtime::{resolve_launcher, ConnectorRunner};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::ApiError;
use crate::AppState;
use gauss_store::{Actor, ActorDefinition, ActorType, NewActor};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateActor {
    pub name: String,
    pub workspace_id: Uuid,
    pub definition_id: Uuid,
    pub configuration: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateActor {
    pub name: Option<String>,
    pub configuration: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilter {
    pub workspace_id: Uuid,
}

/// The JSON Schema configs are split against: the definition's
/// `connectionSpecification` when the registry knows it, else an empty schema
/// (nothing marked secret, nothing extracted).
fn connection_schema(definition: &ActorDefinition) -> Value {
    definition
        .spec
        .as_ref()
        .and_then(|spec| spec.0.get("connectionSpecification").cloned())
        .unwrap_or_else(|| json!({}))
}

async fn store_secrets(state: &AppState, secrets: &[(String, String)]) -> Result<(), ApiError> {
    for (id, value) in secrets {
        state.secrets.put(id, value).await?;
    }
    Ok(())
}

async fn delete_secrets(state: &AppState, config: &Value) -> Result<(), ApiError> {
    for id in gauss_secrets::collect_refs(config) {
        state.secrets.delete(&id).await?;
    }
    Ok(())
}

async fn create(
    state: AppState,
    actor_type: ActorType,
    body: CreateActor,
) -> Result<(StatusCode, Json<Actor>), ApiError> {
    // Validate references up front for precise error messages.
    state.store.workspaces().get(body.workspace_id).await?;
    let definition = state.store.definitions().get(body.definition_id).await?;
    if definition.actor_type != actor_type {
        return Err(ApiError::bad_request(format!(
            "definition `{}` is not a {actor_type:?} definition",
            definition.name
        )));
    }

    let (redacted, secrets) =
        gauss_secrets::split_config(&connection_schema(&definition), &body.configuration);
    store_secrets(&state, &secrets).await?;

    let actor = state
        .store
        .actors()
        .create(&NewActor {
            workspace_id: body.workspace_id,
            definition_id: body.definition_id,
            actor_type,
            name: body.name,
            configuration: redacted,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(actor)))
}

async fn update(
    state: AppState,
    actor_type: ActorType,
    id: Uuid,
    body: UpdateActor,
) -> Result<Json<Actor>, ApiError> {
    let existing = state.store.actors().get(id, actor_type).await?;

    let redacted = match &body.configuration {
        Some(config) => {
            let definition = state
                .store
                .definitions()
                .get(existing.definition_id)
                .await?;
            let (redacted, secrets) =
                gauss_secrets::split_config(&connection_schema(&definition), config);
            store_secrets(&state, &secrets).await?;

            // Drop secrets the new configuration no longer references.
            let kept: Vec<String> = gauss_secrets::collect_refs(&redacted);
            for old_ref in gauss_secrets::collect_refs(&existing.configuration.0) {
                if !kept.contains(&old_ref) {
                    state.secrets.delete(&old_ref).await?;
                }
            }
            Some(redacted)
        }
        None => None,
    };

    let actor = state
        .store
        .actors()
        .update(id, actor_type, body.name.as_deref(), redacted.as_ref())
        .await?;
    Ok(Json(actor))
}

async fn delete(state: AppState, actor_type: ActorType, id: Uuid) -> Result<StatusCode, ApiError> {
    let actor = state.store.actors().get(id, actor_type).await?;
    state.store.actors().delete(id, actor_type).await?;
    delete_secrets(&state, &actor.configuration.0).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stage the actor's hydrated config and build its connector runner.
async fn prepare_runner(
    state: &AppState,
    actor_type: ActorType,
    id: Uuid,
) -> Result<(ConnectorRunner, tempfile::TempDir, std::path::PathBuf), ApiError> {
    let actor = state.store.actors().get(id, actor_type).await?;
    let definition = state.store.definitions().get(actor.definition_id).await?;

    let hydrated =
        gauss_secrets::hydrate_config(&actor.configuration.0, state.secrets.as_ref()).await?;

    let staging = tempfile::tempdir()?;
    let config_path = staging.path().join("config.json");
    tokio::fs::write(&config_path, serde_json::to_vec(&hydrated)?).await?;

    let launcher = resolve_launcher(&definition.docker_repository, &definition.docker_image_tag);
    Ok((ConnectorRunner::new(launcher), staging, config_path))
}

/// Hydrate the stored config and run the connector's `check` operation.
async fn check(
    state: AppState,
    actor_type: ActorType,
    id: Uuid,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (runner, _staging, config_path) = prepare_runner(&state, actor_type, id).await?;
    let status = runner.check(&config_path).await?;
    Ok(Json(serde_json::to_value(&status)?))
}

/// Run the source's `discover` operation: the stream catalog the UI's
/// connection builder selects from.
pub async fn discover_source(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (runner, _staging, config_path) = prepare_runner(&state, ActorType::Source, id).await?;
    let catalog = runner.discover(&config_path).await?;
    Ok(Json(serde_json::to_value(&catalog)?))
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }
}

// Concrete route handlers. axum needs distinct functions per route; these
// pin the actor type and delegate.

macro_rules! actor_routes {
    ($ty:expr, $create:ident, $list:ident, $get:ident, $update:ident, $delete:ident, $check:ident) => {
        pub async fn $create(
            State(state): State<AppState>,
            Json(body): Json<CreateActor>,
        ) -> Result<(StatusCode, Json<Actor>), ApiError> {
            create(state, $ty, body).await
        }

        pub async fn $list(
            State(state): State<AppState>,
            Query(filter): Query<WorkspaceFilter>,
        ) -> Result<Json<serde_json::Value>, ApiError> {
            Ok(super::data(
                state.store.actors().list(filter.workspace_id, $ty).await?,
            ))
        }

        pub async fn $get(
            State(state): State<AppState>,
            Path(id): Path<Uuid>,
        ) -> Result<Json<Actor>, ApiError> {
            Ok(Json(state.store.actors().get(id, $ty).await?))
        }

        pub async fn $update(
            State(state): State<AppState>,
            Path(id): Path<Uuid>,
            Json(body): Json<UpdateActor>,
        ) -> Result<Json<Actor>, ApiError> {
            update(state, $ty, id, body).await
        }

        pub async fn $delete(
            State(state): State<AppState>,
            Path(id): Path<Uuid>,
        ) -> Result<StatusCode, ApiError> {
            delete(state, $ty, id).await
        }

        pub async fn $check(
            State(state): State<AppState>,
            Path(id): Path<Uuid>,
        ) -> Result<Json<serde_json::Value>, ApiError> {
            check(state, $ty, id).await
        }
    };
}

actor_routes!(
    ActorType::Source,
    create_source,
    list_sources,
    get_source,
    update_source,
    delete_source,
    check_source
);

actor_routes!(
    ActorType::Destination,
    create_destination,
    list_destinations,
    get_destination,
    update_destination,
    delete_destination,
    check_destination
);

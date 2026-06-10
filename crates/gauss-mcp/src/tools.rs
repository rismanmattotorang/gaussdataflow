//! Tool catalog and dispatch: each tool is a thin, validated wrapper over
//! the store — the same operations the REST API exposes.

use serde_json::{json, Value};
use uuid::Uuid;

use gauss_connector_runtime::{resolve_launcher, ConnectorRunner};
use gauss_store::{ActorType, NewActor, NewConnection, NewDefinition};

use crate::Gateway;

/// MCP tool descriptors (name, description, JSON-Schema input).
pub fn definitions() -> Value {
    json!([
        {
            "name": "list_workspaces",
            "description": "List all workspaces.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "create_workspace",
            "description": "Create a workspace to hold sources, destinations, and connections.",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }
        },
        {
            "name": "list_connector_definitions",
            "description": "Browse the connector registry. Returns available source or destination connector definitions with their ids and config specs.",
            "inputSchema": {
                "type": "object",
                "properties": {"type": {"type": "string", "enum": ["source", "destination"]}},
                "required": ["type"]
            }
        },
        {
            "name": "register_connector",
            "description": "Register (or update) a connector definition in the registry by docker image, e.g. a connector image compatible with the open Airbyte protocol.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": {"type": "string", "enum": ["source", "destination"]},
                    "name": {"type": "string"},
                    "dockerRepository": {"type": "string", "description": "Image repository, or exec:<path> for a native binary"},
                    "dockerImageTag": {"type": "string"},
                    "documentationUrl": {"type": "string"},
                    "spec": {"type": "object", "description": "Optional connector specification (connectionSpecification JSON Schema)"}
                },
                "required": ["type", "name", "dockerRepository", "dockerImageTag"]
            }
        },
        {
            "name": "create_source",
            "description": "Create a configured source in a workspace. Secret fields (marked airbyte_secret in the spec) are sealed into the secrets backend automatically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspaceId": {"type": "string"},
                    "definitionId": {"type": "string"},
                    "name": {"type": "string"},
                    "configuration": {"type": "object"}
                },
                "required": ["workspaceId", "definitionId", "name", "configuration"]
            }
        },
        {
            "name": "create_destination",
            "description": "Create a configured destination in a workspace (same shape as create_source).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspaceId": {"type": "string"},
                    "definitionId": {"type": "string"},
                    "name": {"type": "string"},
                    "configuration": {"type": "object"}
                },
                "required": ["workspaceId", "definitionId", "name", "configuration"]
            }
        },
        {
            "name": "list_sources",
            "description": "List configured sources in a workspace (configurations are secret-redacted).",
            "inputSchema": {
                "type": "object",
                "properties": {"workspaceId": {"type": "string"}},
                "required": ["workspaceId"]
            }
        },
        {
            "name": "list_destinations",
            "description": "List configured destinations in a workspace.",
            "inputSchema": {
                "type": "object",
                "properties": {"workspaceId": {"type": "string"}},
                "required": ["workspaceId"]
            }
        },
        {
            "name": "check_source",
            "description": "Validate a source's credentials/connectivity by running the connector's check operation.",
            "inputSchema": {
                "type": "object",
                "properties": {"sourceId": {"type": "string"}},
                "required": ["sourceId"]
            }
        },
        {
            "name": "discover_source",
            "description": "Run schema discovery on a source: returns the catalog of streams (names, JSON schemas, supported sync modes) it can replicate.",
            "inputSchema": {
                "type": "object",
                "properties": {"sourceId": {"type": "string"}},
                "required": ["sourceId"]
            }
        },
        {
            "name": "create_connection",
            "description": "Wire a source to a destination with a configured catalog (streams + sync modes). Optionally set a schedule: {\"intervalMinutes\": N} or {\"cron\": \"0 * * * *\"}.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "sourceId": {"type": "string"},
                    "destinationId": {"type": "string"},
                    "catalog": {"type": "object", "description": "Configured catalog: {streams: [{stream, sync_mode, destination_sync_mode, …}]}"},
                    "schedule": {"type": "object"}
                },
                "required": ["name", "sourceId", "destinationId", "catalog"]
            }
        },
        {
            "name": "list_connections",
            "description": "List connections in a workspace.",
            "inputSchema": {
                "type": "object",
                "properties": {"workspaceId": {"type": "string"}},
                "required": ["workspaceId"]
            }
        },
        {
            "name": "trigger_sync",
            "description": "Enqueue a sync job for a connection. Fails with a conflict if one is already pending or running.",
            "inputSchema": {
                "type": "object",
                "properties": {"connectionId": {"type": "string"}},
                "required": ["connectionId"]
            }
        },
        {
            "name": "list_jobs",
            "description": "List recent jobs for a connection (newest first).",
            "inputSchema": {
                "type": "object",
                "properties": {"connectionId": {"type": "string"}},
                "required": ["connectionId"]
            }
        },
        {
            "name": "get_job",
            "description": "Get a job with its attempts (status, records synced, timestamps).",
            "inputSchema": {
                "type": "object",
                "properties": {"jobId": {"type": "integer"}},
                "required": ["jobId"]
            }
        },
        {
            "name": "cancel_job",
            "description": "Cancel a job: pending jobs stop immediately, running jobs stop at the next message boundary.",
            "inputSchema": {
                "type": "object",
                "properties": {"jobId": {"type": "integer"}},
                "required": ["jobId"]
            }
        },
        {
            "name": "get_connection_state",
            "description": "Read a connection's committed sync state (per-stream cursors).",
            "inputSchema": {
                "type": "object",
                "properties": {"connectionId": {"type": "string"}},
                "required": ["connectionId"]
            }
        }
    ])
}

fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing required argument `{key}`"))
}

fn arg_uuid(args: &Value, key: &str) -> Result<Uuid, String> {
    Uuid::parse_str(&arg_str(args, key)?).map_err(|e| format!("`{key}` is not a UUID: {e}"))
}

fn arg_i64(args: &Value, key: &str) -> Result<i64, String> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing required integer argument `{key}`"))
}

fn actor_type(args: &Value) -> Result<ActorType, String> {
    match args.get("type").and_then(Value::as_str) {
        Some("source") => Ok(ActorType::Source),
        Some("destination") => Ok(ActorType::Destination),
        _ => Err("`type` must be \"source\" or \"destination\"".to_string()),
    }
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

impl Gateway {
    pub(crate) async fn call_tool(&self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "list_workspaces" => {
                serde_json::to_value(self.store.workspaces().list().await.map_err(err)?)
                    .map_err(err)
            }
            "create_workspace" => serde_json::to_value(
                self.store
                    .workspaces()
                    .create(&arg_str(&args, "name")?)
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "list_connector_definitions" => serde_json::to_value(
                self.store
                    .definitions()
                    .list(actor_type(&args)?)
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "register_connector" => serde_json::to_value(
                self.store
                    .definitions()
                    .upsert(&NewDefinition {
                        id: Uuid::new_v4(),
                        actor_type: actor_type(&args)?,
                        name: arg_str(&args, "name")?,
                        docker_repository: arg_str(&args, "dockerRepository")?,
                        docker_image_tag: arg_str(&args, "dockerImageTag")?,
                        documentation_url: args
                            .get("documentationUrl")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        spec: args.get("spec").filter(|s| !s.is_null()).cloned(),
                    })
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "create_source" => self.create_actor(ActorType::Source, &args).await,
            "create_destination" => self.create_actor(ActorType::Destination, &args).await,
            "list_sources" => serde_json::to_value(
                self.store
                    .actors()
                    .list(arg_uuid(&args, "workspaceId")?, ActorType::Source)
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "list_destinations" => serde_json::to_value(
                self.store
                    .actors()
                    .list(arg_uuid(&args, "workspaceId")?, ActorType::Destination)
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "check_source" => {
                let (runner, _staging, config) = self
                    .prepare(arg_uuid(&args, "sourceId")?, ActorType::Source)
                    .await?;
                serde_json::to_value(runner.check(&config).await.map_err(err)?).map_err(err)
            }
            "discover_source" => {
                let (runner, _staging, config) = self
                    .prepare(arg_uuid(&args, "sourceId")?, ActorType::Source)
                    .await?;
                serde_json::to_value(runner.discover(&config).await.map_err(err)?).map_err(err)
            }
            "create_connection" => {
                let source = self
                    .store
                    .actors()
                    .get(arg_uuid(&args, "sourceId")?, ActorType::Source)
                    .await
                    .map_err(err)?;
                let destination = self
                    .store
                    .actors()
                    .get(arg_uuid(&args, "destinationId")?, ActorType::Destination)
                    .await
                    .map_err(err)?;
                if source.workspace_id != destination.workspace_id {
                    return Err("source and destination belong to different workspaces".into());
                }
                serde_json::to_value(
                    self.store
                        .connections()
                        .create(&NewConnection {
                            workspace_id: source.workspace_id,
                            source_id: source.id,
                            destination_id: destination.id,
                            name: arg_str(&args, "name")?,
                            catalog: args
                                .get("catalog")
                                .cloned()
                                .ok_or("missing required argument `catalog`")?,
                            schedule: args.get("schedule").filter(|s| !s.is_null()).cloned(),
                        })
                        .await
                        .map_err(err)?,
                )
                .map_err(err)
            }
            "list_connections" => serde_json::to_value(
                self.store
                    .connections()
                    .list(arg_uuid(&args, "workspaceId")?)
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "trigger_sync" => serde_json::to_value(
                self.store
                    .jobs()
                    .create(arg_uuid(&args, "connectionId")?, "sync")
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "list_jobs" => serde_json::to_value(
                self.store
                    .jobs()
                    .list(arg_uuid(&args, "connectionId")?)
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "get_job" => {
                let id = arg_i64(&args, "jobId")?;
                let job = self.store.jobs().get(id).await.map_err(err)?;
                let attempts = self.store.jobs().list_attempts(id).await.map_err(err)?;
                let mut value = serde_json::to_value(&job).map_err(err)?;
                value["attempts"] = serde_json::to_value(&attempts).map_err(err)?;
                Ok(value)
            }
            "cancel_job" => serde_json::to_value(
                self.store
                    .jobs()
                    .cancel(arg_i64(&args, "jobId")?)
                    .await
                    .map_err(err)?,
            )
            .map_err(err),
            "get_connection_state" => Ok(json!({
                "state": self
                    .store
                    .connection_states()
                    .get(arg_uuid(&args, "connectionId")?)
                    .await
                    .map_err(err)?,
            })),
            other => Err(format!("unknown tool `{other}`")),
        }
    }

    async fn create_actor(&self, actor_type: ActorType, args: &Value) -> Result<Value, String> {
        let definition = self
            .store
            .definitions()
            .get(arg_uuid(args, "definitionId")?)
            .await
            .map_err(err)?;
        if definition.actor_type != actor_type {
            return Err(format!(
                "definition `{}` is not a {actor_type:?} definition",
                definition.name
            ));
        }
        let configuration = args
            .get("configuration")
            .cloned()
            .ok_or("missing required argument `configuration`")?;

        let schema = definition
            .spec
            .as_ref()
            .and_then(|s| s.0.get("connectionSpecification").cloned())
            .unwrap_or_else(|| json!({}));
        let (redacted, secrets) = gauss_secrets::split_config(&schema, &configuration);
        for (id, value) in &secrets {
            self.secrets.put(id, value).await.map_err(err)?;
        }

        serde_json::to_value(
            self.store
                .actors()
                .create(&NewActor {
                    workspace_id: arg_uuid(args, "workspaceId")?,
                    definition_id: definition.id,
                    actor_type,
                    name: arg_str(args, "name")?,
                    configuration: redacted,
                })
                .await
                .map_err(err)?,
        )
        .map_err(err)
    }

    /// Hydrate + stage a source config and build its runner (for
    /// check/discover tools).
    async fn prepare(
        &self,
        id: Uuid,
        actor_type: ActorType,
    ) -> Result<(ConnectorRunner, tempfile::TempDir, std::path::PathBuf), String> {
        let actor = self.store.actors().get(id, actor_type).await.map_err(err)?;
        let definition = self
            .store
            .definitions()
            .get(actor.definition_id)
            .await
            .map_err(err)?;
        let hydrated = gauss_secrets::hydrate_config(&actor.configuration.0, self.secrets.as_ref())
            .await
            .map_err(err)?;

        let staging = tempfile::tempdir().map_err(err)?;
        let config = staging.path().join("config.json");
        std::fs::write(&config, serde_json::to_vec(&hydrated).map_err(err)?).map_err(err)?;

        let launcher =
            resolve_launcher(&definition.docker_repository, &definition.docker_image_tag);
        Ok((ConnectorRunner::new(launcher), staging, config))
    }
}

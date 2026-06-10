//! MCP gateway: the gaussdataflow control plane as Model Context Protocol
//! tools.
//!
//! Speaks JSON-RPC 2.0 over stdio (the standard MCP transport), so any MCP
//! client — Claude Desktop, Claude Code, agent frameworks — can manage
//! workspaces, connectors, connections, and syncs conversationally:
//!
//! ```json
//! { "mcpServers": { "gaussdataflow": {
//!     "command": "gauss-mcp",
//!     "env": { "DATABASE_URL": "postgres://…" } } } }
//! ```
//!
//! The gateway talks to the same store and secrets backend as the API
//! server; configurations created here get the same secret envelope, and
//! `check_source`/`discover_source` launch real connectors.

mod tools;

use serde_json::{json, Value};

use gauss_store::Store;

pub const PROTOCOL_VERSION: &str = "2024-11-05";

pub struct Gateway {
    pub(crate) store: Store,
    pub(crate) secrets: std::sync::Arc<dyn gauss_secrets::SecretsBackend>,
}

impl Gateway {
    pub fn new(store: Store) -> Self {
        let secrets = std::sync::Arc::new(store.secrets_backend());
        Self { store, secrets }
    }

    /// Handle one JSON-RPC message. Returns `None` for notifications (which
    /// take no response).
    pub async fn handle(&self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str)?;

        // Notifications (no id) are acknowledged silently.
        let id = match id {
            Some(id) if !id.is_null() => id,
            _ => return None,
        };

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": message["params"]["protocolVersion"]
                    .as_str()
                    .unwrap_or(PROTOCOL_VERSION),
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "gaussdataflow",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": "Manage gaussdataflow data pipelines: browse the \
                    connector registry, configure sources and destinations, wire \
                    connections, trigger and monitor syncs.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::definitions() })),
            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                let args = message["params"]["arguments"].clone();
                match self.call_tool(name, args).await {
                    Ok(value) => Ok(json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&value)
                                .unwrap_or_else(|_| value.to_string()),
                        }],
                        "isError": false,
                    })),
                    Err(message) => Ok(json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": true,
                    })),
                }
            }
            other => Err(json!({
                "code": -32601,
                "message": format!("method `{other}` not found"),
            })),
        };

        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        })
    }
}

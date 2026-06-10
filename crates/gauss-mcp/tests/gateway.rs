//! MCP gateway integration tests: full JSON-RPC lifecycle against a real
//! Postgres, driving the same flow an AI agent would — registry → source →
//! destination → connection → sync job. Skips without `DATABASE_URL`.

use gauss_mcp::Gateway;
use gauss_store::Store;
use serde_json::{json, Value};

async fn gateway() -> Option<Gateway> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping");
        return None;
    };
    let admin = sqlx::PgPool::connect(&url).await.expect("admin connect");
    let name = format!("gauss_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
        .execute(&admin)
        .await
        .expect("create test database");
    let (base, _) = url.rsplit_once('/').unwrap();
    Some(Gateway::new(
        Store::connect(&format!("{base}/{name}")).await.unwrap(),
    ))
}

async fn rpc(gateway: &Gateway, id: i64, method: &str, params: Value) -> Value {
    let response = gateway
        .handle(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
        .await
        .expect("request with id gets a response");
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], id);
    response
}

/// Call a tool and parse the JSON payload out of the MCP content envelope.
async fn call(gateway: &Gateway, name: &str, args: Value) -> Result<Value, String> {
    let response = rpc(
        gateway,
        7,
        "tools/call",
        json!({"name": name, "arguments": args}),
    )
    .await;
    let result = &response["result"];
    let text = result["content"][0]["text"].as_str().unwrap().to_string();
    if result["isError"].as_bool() == Some(true) {
        Err(text)
    } else {
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

#[tokio::test]
async fn initialize_and_list_tools() {
    let Some(gw) = gateway().await else { return };

    let init = rpc(
        &gw,
        1,
        "initialize",
        json!({"protocolVersion": "2024-11-05", "capabilities": {}}),
    )
    .await;
    assert_eq!(init["result"]["serverInfo"]["name"], "gaussdataflow");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");

    // notifications get no response
    assert!(gw
        .handle(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await
        .is_none());

    let tools = rpc(&gw, 2, "tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "create_workspace",
        "register_connector",
        "create_source",
        "create_connection",
        "trigger_sync",
        "get_job",
        "cancel_job",
        "get_connection_state",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    let unknown = rpc(&gw, 3, "no/such/method", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);
}

#[tokio::test]
async fn agent_flow_end_to_end() {
    let Some(gw) = gateway().await else { return };

    let ws = call(&gw, "create_workspace", json!({"name": "agent-ws"}))
        .await
        .unwrap();
    let ws_id = ws["workspaceId"].as_str().unwrap().to_string();

    // Register a source definition whose spec marks `token` secret.
    let spec = json!({"connectionSpecification": {
        "type": "object",
        "properties": {"token": {"type": "string", "airbyte_secret": true}}
    }});
    let src_def = call(
        &gw,
        "register_connector",
        json!({
            "type": "source", "name": "Test API", "dockerRepository": "example/source-test",
            "dockerImageTag": "1.0", "spec": spec
        }),
    )
    .await
    .unwrap();
    let dst_def = call(
        &gw,
        "register_connector",
        json!({
            "type": "destination", "name": "Test Warehouse",
            "dockerRepository": "example/destination-test", "dockerImageTag": "1.0"
        }),
    )
    .await
    .unwrap();

    let listed = call(&gw, "list_connector_definitions", json!({"type": "source"}))
        .await
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    // Secrets are sealed: the raw token never appears in tool output.
    let source = call(
        &gw,
        "create_source",
        json!({
            "workspaceId": ws_id, "definitionId": src_def["definitionId"],
            "name": "api src", "configuration": {"token": "sk-very-secret"}
        }),
    )
    .await
    .unwrap();
    assert!(!source.to_string().contains("sk-very-secret"));
    assert!(source["configuration"]["token"]["_secret"].is_string());

    let destination = call(
        &gw,
        "create_destination",
        json!({
            "workspaceId": ws_id, "definitionId": dst_def["definitionId"],
            "name": "wh dst", "configuration": {}
        }),
    )
    .await
    .unwrap();

    let connection = call(
        &gw,
        "create_connection",
        json!({
            "name": "api → warehouse",
            "sourceId": source["id"], "destinationId": destination["id"],
            "catalog": {"streams": [{
                "stream": {"name": "events", "json_schema": {}},
                "sync_mode": "full_refresh", "destination_sync_mode": "append"
            }]},
            "schedule": {"intervalMinutes": 60}
        }),
    )
    .await
    .unwrap();
    let conn_id = connection["connectionId"].as_str().unwrap().to_string();

    // Trigger, inspect, duplicate-reject, cancel.
    let job = call(&gw, "trigger_sync", json!({"connectionId": conn_id}))
        .await
        .unwrap();
    assert_eq!(job["status"], "pending");
    let dup = call(&gw, "trigger_sync", json!({"connectionId": conn_id})).await;
    assert!(dup.is_err(), "duplicate trigger must error");

    let fetched = call(&gw, "get_job", json!({"jobId": job["id"]}))
        .await
        .unwrap();
    assert_eq!(fetched["attempts"].as_array().unwrap().len(), 0);

    let cancelled = call(&gw, "cancel_job", json!({"jobId": job["id"]}))
        .await
        .unwrap();
    assert_eq!(cancelled["status"], "cancelled");

    let state = call(
        &gw,
        "get_connection_state",
        json!({"connectionId": conn_id}),
    )
    .await
    .unwrap();
    assert!(state["state"].is_null());

    // Bad arguments surface as tool errors, not crashes.
    let bad = call(&gw, "create_source", json!({"workspaceId": "not-a-uuid"})).await;
    assert!(bad.is_err());
    let unknown = call(&gw, "definitely_not_a_tool", json!({})).await;
    assert!(unknown.is_err());
}

//! API integration tests against a real Postgres.
//!
//! Each test creates a throwaway database from `DATABASE_URL` (any admin
//! connection string) and runs migrations into it. Without `DATABASE_URL`
//! the tests skip, so contributors without Postgres still get a green
//! `cargo test`; CI always provides the database.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use gauss_server::AppState;
use gauss_store::Store;
use serde_json::{json, Value};
use tower::util::ServiceExt;

async fn test_state() -> Option<AppState> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping DB-backed test");
        return None;
    };
    let admin = sqlx::PgPool::connect(&url).await.expect("admin connect");
    let name = format!("gauss_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
        .execute(&admin)
        .await
        .expect("create test database");
    let (base, _) = url.rsplit_once('/').expect("database url with db name");
    let store = Store::connect(&format!("{base}/{name}"))
        .await
        .expect("connect + migrate test database");
    Some(AppState::new(store))
}

async fn req(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(json) => {
            builder = builder.header("content-type", "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// Registry document with a source/destination pair whose spec marks
/// `password` as secret.
fn registry_with_secret_spec() -> Value {
    let spec = json!({
        "connectionSpecification": {
            "type": "object",
            "properties": {
                "host": {"type": "string"},
                "password": {"type": "string", "gauss_secret": true}
            }
        }
    });
    json!({
        "sources": [{
            "definitionId": "11111111-1111-4111-8111-111111111111",
            "name": "Test Source",
            "dockerRepository": "example/source-test",
            "dockerImageTag": "1.0",
            "spec": spec
        }],
        "destinations": [{
            "definitionId": "22222222-2222-4222-8222-222222222222",
            "name": "Test Destination",
            "dockerRepository": "example/destination-test",
            "dockerImageTag": "1.0",
            "spec": spec
        }]
    })
}

fn configured_catalog() -> Value {
    json!({
        "streams": [{
            "stream": {"name": "users", "json_schema": {"type": "object"}},
            "sync_mode": "full_refresh",
            "destination_sync_mode": "append"
        }]
    })
}

/// Boots a workspace + definitions + one source and destination; returns
/// (app, state, workspace_id, source_id, destination_id).
async fn seed_actors(state: &AppState) -> (Router, String, String, String) {
    let app = gauss_server::app(state.clone());

    let (status, ws) = req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(json!({"name": "acme"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ws_id = ws["workspaceId"].as_str().unwrap().to_string();

    let (status, _) = req(
        &app,
        "POST",
        "/api/v1/definitions/import",
        Some(registry_with_secret_spec()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, source) = req(
        &app,
        "POST",
        "/api/v1/sources",
        Some(json!({
            "name": "test source",
            "workspaceId": ws_id,
            "definitionId": "11111111-1111-4111-8111-111111111111",
            "configuration": {"host": "db.internal", "password": "s3cret"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{source}");
    let source_id = source["id"].as_str().unwrap().to_string();

    let (status, dest) = req(
        &app,
        "POST",
        "/api/v1/destinations",
        Some(json!({
            "name": "test destination",
            "workspaceId": ws_id,
            "definitionId": "22222222-2222-4222-8222-222222222222",
            "configuration": {"host": "warehouse.internal", "password": "w4rehouse"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{dest}");
    let dest_id = dest["id"].as_str().unwrap().to_string();

    (app, ws_id, source_id, dest_id)
}

#[tokio::test]
async fn health() {
    let Some(state) = test_state().await else {
        return;
    };
    let app = gauss_server::app(state);
    let (status, body) = req(&app, "GET", "/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn workspace_crud() {
    let Some(state) = test_state().await else {
        return;
    };
    let app = gauss_server::app(state);

    let (status, _) = req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(json!({"name": "  "})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, ws) = req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(json!({"name": "acme"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = ws["workspaceId"].as_str().unwrap();

    let (status, list) = req(&app, "GET", "/api/v1/workspaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"].as_array().unwrap().len(), 1);

    let (status, got) = req(&app, "GET", &format!("/api/v1/workspaces/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["name"], "acme");

    let (status, _) = req(&app, "DELETE", &format!("/api/v1/workspaces/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = req(&app, "GET", &format!("/api/v1/workspaces/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn registry_import_is_idempotent() {
    let Some(state) = test_state().await else {
        return;
    };
    let app = gauss_server::app(state);
    let seed: Value = serde_json::from_str(include_str!("../seed/registry.json")).unwrap();

    let n_sources = seed["sources"].as_array().unwrap().len();
    let n_destinations = seed["destinations"].as_array().unwrap().len();

    for _ in 0..2 {
        let (status, summary) = req(
            &app,
            "POST",
            "/api/v1/definitions/import",
            Some(seed.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(summary["sourcesImported"], n_sources);
        assert_eq!(summary["destinationsImported"], n_destinations);
    }

    let (_, sources) = req(&app, "GET", "/api/v1/source_definitions", None).await;
    assert_eq!(sources["data"].as_array().unwrap().len(), n_sources);
    let (_, dests) = req(&app, "GET", "/api/v1/destination_definitions", None).await;
    assert_eq!(dests["data"].as_array().unwrap().len(), n_destinations);
}

#[tokio::test]
async fn source_secrets_are_redacted_and_cleaned_up() {
    let Some(state) = test_state().await else {
        return;
    };
    let (app, _ws, source_id, _dest) = seed_actors(&state).await;

    // The API must never return the raw secret.
    let (status, source) = req(&app, "GET", &format!("/api/v1/sources/{source_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!source.to_string().contains("s3cret"));
    let secret_ref = source["configuration"]["password"]["_secret"]
        .as_str()
        .unwrap()
        .to_string();

    // The raw value is in the backend, hydratable server-side.
    assert_eq!(state.secrets.get(&secret_ref).await.unwrap(), "s3cret");

    // Round-tripping the redacted config in an update preserves the ref.
    let (status, updated) = req(
        &app,
        "PATCH",
        &format!("/api/v1/sources/{source_id}"),
        Some(json!({
            "configuration": {"host": "db2.internal", "password": {"_secret": secret_ref}}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["configuration"]["host"], "db2.internal");
    assert_eq!(
        updated["configuration"]["password"]["_secret"],
        secret_ref.as_str()
    );

    // Supplying a new raw password rotates the secret: new ref, old deleted.
    let (status, rotated) = req(
        &app,
        "PATCH",
        &format!("/api/v1/sources/{source_id}"),
        Some(json!({"configuration": {"host": "db2.internal", "password": "n3w"}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let new_ref = rotated["configuration"]["password"]["_secret"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(new_ref, secret_ref);
    assert!(state.secrets.get(&secret_ref).await.is_err());
    assert_eq!(state.secrets.get(&new_ref).await.unwrap(), "n3w");

    // Deleting the source deletes its secrets.
    let (status, _) = req(
        &app,
        "DELETE",
        &format!("/api/v1/sources/{source_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(state.secrets.get(&new_ref).await.is_err());
}

#[tokio::test]
async fn wrong_definition_type_is_rejected() {
    let Some(state) = test_state().await else {
        return;
    };
    let (app, ws_id, _source, _dest) = seed_actors(&state).await;

    // Try to create a source from a destination definition.
    let (status, body) = req(
        &app,
        "POST",
        "/api/v1/sources",
        Some(json!({
            "name": "bad",
            "workspaceId": ws_id,
            "definitionId": "22222222-2222-4222-8222-222222222222",
            "configuration": {}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn connection_lifecycle() {
    let Some(state) = test_state().await else {
        return;
    };
    let (app, ws_id, source_id, dest_id) = seed_actors(&state).await;

    // Invalid catalog rejected up front.
    let (status, body) = req(
        &app,
        "POST",
        "/api/v1/connections",
        Some(json!({
            "name": "bad",
            "sourceId": source_id,
            "destinationId": dest_id,
            "catalog": {"streams": [{"stream": {"name": "x"}, "sync_mode": "bogus"}]}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, conn) = req(
        &app,
        "POST",
        "/api/v1/connections",
        Some(json!({
            "name": "users sync",
            "sourceId": source_id,
            "destinationId": dest_id,
            "catalog": configured_catalog(),
            "schedule": {"cron": "0 * * * *"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{conn}");
    let conn_id = conn["connectionId"].as_str().unwrap().to_string();
    assert_eq!(conn["workspaceId"].as_str().unwrap(), ws_id);
    assert_eq!(conn["status"], "active");

    let (status, list) = req(
        &app,
        "GET",
        &format!("/api/v1/connections?workspaceId={ws_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"].as_array().unwrap().len(), 1);

    let (status, patched) = req(
        &app,
        "PATCH",
        &format!("/api/v1/connections/{conn_id}"),
        Some(json!({"status": "inactive"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["status"], "inactive");

    // Deleting the source cascades to the connection.
    let (status, _) = req(
        &app,
        "DELETE",
        &format!("/api/v1/sources/{source_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = req(&app, "GET", &format!("/api/v1/connections/{conn_id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cross_workspace_connection_is_rejected() {
    let Some(state) = test_state().await else {
        return;
    };
    let (app, _ws, source_id, _dest) = seed_actors(&state).await;

    // Destination in a different workspace.
    let (_, other_ws) = req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(json!({"name": "other"})),
    )
    .await;
    let (status, other_dest) = req(
        &app,
        "POST",
        "/api/v1/destinations",
        Some(json!({
            "name": "elsewhere",
            "workspaceId": other_ws["workspaceId"],
            "definitionId": "22222222-2222-4222-8222-222222222222",
            "configuration": {}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = req(
        &app,
        "POST",
        "/api/v1/connections",
        Some(json!({
            "name": "cross",
            "sourceId": source_id,
            "destinationId": other_dest["id"],
            "catalog": configured_catalog()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// Full bridge to the Phase-1 runtime: POST /sources/{id}/check launches the
/// connector image via Docker with the hydrated config. Requires Docker and
/// the locally built `gauss/mock-source:dev` image; gated behind an env var
/// so plain `cargo test` stays hermetic.
#[tokio::test]
async fn check_endpoint_runs_connector() {
    if std::env::var("GAUSS_DOCKER_E2E").is_err() {
        eprintln!("GAUSS_DOCKER_E2E not set; skipping docker-backed check test");
        return;
    }
    let Some(state) = test_state().await else {
        return;
    };
    let app = gauss_server::app(state.clone());

    let (_, ws) = req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(json!({"name": "e2e"})),
    )
    .await;
    let seed: Value = serde_json::from_str(include_str!("../seed/registry.json")).unwrap();
    req(&app, "POST", "/api/v1/definitions/import", Some(seed)).await;

    let (status, source) = req(
        &app,
        "POST",
        "/api/v1/sources",
        Some(json!({
            "name": "mock",
            "workspaceId": ws["workspaceId"],
            "definitionId": "0c2f4a3a-0000-4000-8000-000000000001",
            "configuration": {"record_count": 1}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{source}");

    let (status, result) = req(
        &app,
        "POST",
        &format!("/api/v1/sources/{}/check", source["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "SUCCEEDED");
}

#[tokio::test]
async fn sync_trigger_and_cancel_via_api() {
    let Some(state) = test_state().await else {
        return;
    };
    let (app, _ws, source_id, dest_id) = seed_actors(&state).await;

    let (status, conn) = req(
        &app,
        "POST",
        "/api/v1/connections",
        Some(json!({
            "name": "users sync",
            "sourceId": source_id,
            "destinationId": dest_id,
            "catalog": configured_catalog()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let conn_id = conn["connectionId"].as_str().unwrap().to_string();

    // Trigger a sync job.
    let (status, job) = req(
        &app,
        "POST",
        &format!("/api/v1/connections/{conn_id}/sync"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{job}");
    let job_id = job["id"].as_i64().unwrap();
    assert_eq!(job["status"], "pending");
    assert_eq!(job["jobType"], "sync");

    // Duplicate trigger while pending → 409.
    let (status, _) = req(
        &app,
        "POST",
        &format!("/api/v1/connections/{conn_id}/sync"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Job is listed and fetchable with attempts.
    let (status, list) = req(
        &app,
        "GET",
        &format!("/api/v1/connections/{conn_id}/jobs"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"].as_array().unwrap().len(), 1);
    let (status, fetched) = req(&app, "GET", &format!("/api/v1/jobs/{job_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(fetched["attempts"].as_array().unwrap().is_empty());

    // No state yet.
    let (status, body) = req(
        &app,
        "GET",
        &format!("/api/v1/connections/{conn_id}/state"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["state"].is_null());

    // Cancel the pending job; re-cancel conflicts.
    let (status, cancelled) =
        req(&app, "POST", &format!("/api/v1/jobs/{job_id}/cancel"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "cancelled");
    let (status, _) = req(&app, "POST", &format!("/api/v1/jobs/{job_id}/cancel"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Inactive connections cannot be triggered.
    req(
        &app,
        "PATCH",
        &format!("/api/v1/connections/{conn_id}"),
        Some(json!({"status": "inactive"})),
    )
    .await;
    let (status, _) = req(
        &app,
        "POST",
        &format!("/api/v1/connections/{conn_id}/sync"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn fleet_stats_and_recent_jobs() {
    let Some(state) = test_state().await else {
        return;
    };
    let (app, ws_id, source_id, dest_id) = seed_actors(&state).await;

    // Empty fleet: stats exist, activity feed is empty.
    let (status, stats) = req(&app, "GET", "/api/v1/stats", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["connections"], 0);
    assert_eq!(stats["sources"], 1);
    assert_eq!(stats["destinations"], 1);
    let (status, jobs) = req(&app, "GET", "/api/v1/jobs?limit=10", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(jobs["data"].as_array().unwrap().is_empty());

    // One connection + one triggered sync show up everywhere.
    let (status, conn) = req(
        &app,
        "POST",
        "/api/v1/connections",
        Some(json!({
            "name": "orders → warehouse",
            "sourceId": source_id,
            "destinationId": dest_id,
            "catalog": configured_catalog()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let conn_id = conn["connectionId"].as_str().unwrap().to_string();
    let (status, job) = req(
        &app,
        "POST",
        &format!("/api/v1/connections/{conn_id}/sync"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, stats) = req(
        &app,
        "GET",
        &format!("/api/v1/stats?workspaceId={ws_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["connections"], 1);
    assert_eq!(stats["jobsPending"], 1);
    assert_eq!(stats["jobsRunning"], 0);

    let (status, recent) = req(
        &app,
        "GET",
        &format!("/api/v1/jobs?workspaceId={ws_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries = recent["data"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], job["id"]);
    assert_eq!(entries[0]["connectionName"], "orders → warehouse");
    assert_eq!(entries[0]["status"], "pending");

    // Scoping to another workspace filters everything out.
    let other = uuid::Uuid::new_v4();
    let (_, scoped) = req(
        &app,
        "GET",
        &format!("/api/v1/jobs?workspaceId={other}"),
        None,
    )
    .await;
    assert!(scoped["data"].as_array().unwrap().is_empty());
    let (_, scoped) = req(
        &app,
        "GET",
        &format!("/api/v1/stats?workspaceId={other}"),
        None,
    )
    .await;
    assert_eq!(scoped["connections"], 0);
}

#[tokio::test]
async fn cors_is_pinned_when_origins_are_configured() {
    let Some(state) = test_state().await else {
        return;
    };
    let app =
        gauss_server::app(state.cors_origins(vec!["http://console.example".parse().unwrap()]));

    // The configured console origin is allowed…
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .header("origin", "http://console.example")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "http://console.example"
    );

    // …any other origin is not.
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .header("origin", "http://elsewhere.example")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert!(response
        .headers()
        .get("access-control-allow-origin")
        .is_none());
}

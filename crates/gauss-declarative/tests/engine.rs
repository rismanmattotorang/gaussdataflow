//! Declarative engine tests against a local HTTP API: auth enforcement,
//! offset + cursor pagination, incremental resume, and the compiled
//! `gauss-declarative` binary driven over the real wire protocol.

use axum::extract::Query;
use axum::http::HeaderMap;
use axum::routing::get;
use axum::Json;
use gauss_cdk::{Emitter, Source};
use gauss_declarative::DeclarativeSource;
use serde_json::{json, Value};

/// Local API: /users (offset-paginated, api-key auth, 7 records with
/// updated_at cursors) and /events (cursor-token pagination).
async fn serve_api() -> String {
    async fn users(
        headers: HeaderMap,
        Query(q): Query<Value>,
    ) -> Result<Json<Value>, axum::http::StatusCode> {
        if headers.get("x-api-key").and_then(|v| v.to_str().ok()) != Some("k-123") {
            return Err(axum::http::StatusCode::UNAUTHORIZED);
        }
        let offset: usize = q["offset"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let limit: usize = q["limit"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);
        let all: Vec<Value> = (1..=7)
            .map(|i| json!({"id": i, "updated_at": format!("2026-01-0{i}T00:00:00Z")}))
            .collect();
        let page: Vec<Value> = all.into_iter().skip(offset).take(limit).collect();
        Ok(Json(json!({"data": page})))
    }

    async fn events(Query(q): Query<Value>) -> Json<Value> {
        // Two pages chained by a `next` token.
        match q.get("after").and_then(Value::as_str) {
            None => Json(json!({"items": [{"id": "e1"}, {"id": "e2"}], "next": "tok-2"})),
            Some("tok-2") => Json(json!({"items": [{"id": "e3"}], "next": null})),
            Some(_) => Json(json!({"items": [], "next": null})),
        }
    }

    let app = axum::Router::new()
        .route("/users", get(users))
        .route("/events", get(events));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn manifest(url_base: &str) -> Value {
    json!({
        "requester": {
            "url_base": url_base,
            "authenticator": {"type": "api_key", "header": "X-Api-Key", "api_token": "{{ config.api_key }}"}
        },
        "check": {"stream": "users"},
        "streams": [
            {
                "name": "users",
                "path": "/users",
                "record_selector": "data",
                "primary_key": ["id"],
                "cursor_field": "updated_at",
                "paginator": {"type": "offset", "page_size": 3}
            },
            {
                "name": "events",
                "path": "/events",
                "record_selector": "items",
                "paginator": {"type": "cursor", "cursor_path": "next", "cursor_param": "after"}
            }
        ],
        "spec": {
            "connection_specification": {
                "type": "object",
                "required": ["api_key"],
                "properties": {"api_key": {"type": "string", "gauss_secret": true}}
            }
        }
    })
}

fn catalog(streams: &[(&str, &str)]) -> gauss_cdk::protocol::ConfiguredGaussCatalog {
    serde_json::from_value(json!({
        "streams": streams.iter().map(|(name, mode)| json!({
            "stream": {"name": name, "json_schema": {}},
            "sync_mode": mode,
            "destination_sync_mode": "append"
        })).collect::<Vec<_>>()
    }))
    .unwrap()
}

async fn read(
    config: &Value,
    streams: &[(&str, &str)],
    state: Option<&Value>,
) -> Vec<gauss_cdk::protocol::GaussMessage> {
    let (mut emitter, buffer) = Emitter::buffer();
    DeclarativeSource
        .read(config, &catalog(streams), state, &mut emitter)
        .await
        .expect("read succeeds");
    Emitter::parse_buffer(&buffer)
}

fn records(messages: &[gauss_cdk::protocol::GaussMessage]) -> Vec<&Value> {
    messages
        .iter()
        .filter_map(|m| m.record.as_ref().map(|r| &r.data))
        .collect()
}

#[tokio::test]
async fn check_validates_credentials() {
    let base = serve_api().await;
    let good = json!({"manifest": manifest(&base), "api_key": "k-123"});
    let status = DeclarativeSource.check(&good).await.unwrap();
    assert_eq!(
        status.status,
        gauss_cdk::protocol::ConnectionStatus::Succeeded
    );

    let bad = json!({"manifest": manifest(&base), "api_key": "wrong"});
    let status = DeclarativeSource.check(&bad).await.unwrap();
    assert_eq!(status.status, gauss_cdk::protocol::ConnectionStatus::Failed);
    assert!(status.message.unwrap().contains("401"));
}

#[tokio::test]
async fn discover_reflects_manifest() {
    let base = serve_api().await;
    let config = json!({"manifest": manifest(&base), "api_key": "k-123"});
    let catalog = DeclarativeSource.discover(&config).await.unwrap();
    assert_eq!(catalog.streams.len(), 2);

    let users = &catalog.streams[0];
    assert_eq!(users.name, "users");
    assert_eq!(users.default_cursor_field, Some(vec!["updated_at".into()]));
    assert_eq!(
        users.source_defined_primary_key,
        Some(vec![vec!["id".into()]])
    );
    assert_eq!(
        users.supported_sync_modes.as_ref().unwrap().len(),
        2,
        "cursored stream supports incremental"
    );
    assert_eq!(
        catalog.streams[1]
            .supported_sync_modes
            .as_ref()
            .unwrap()
            .len(),
        1,
        "cursorless stream is full-refresh only"
    );
}

#[tokio::test]
async fn offset_pagination_reads_all_pages() {
    let base = serve_api().await;
    let config = json!({"manifest": manifest(&base), "api_key": "k-123"});
    let messages = read(&config, &[("users", "incremental")], None).await;

    // 7 records across pages of 3 (3 + 3 + 1).
    assert_eq!(records(&messages).len(), 7);

    // One checkpoint at the high-water mark.
    let states: Vec<_> = messages.iter().filter_map(|m| m.state.as_ref()).collect();
    assert_eq!(states.len(), 1);
    let cursor = states[0]
        .stream
        .as_ref()
        .unwrap()
        .stream_state
        .as_ref()
        .unwrap();
    assert_eq!(cursor["updated_at"], "2026-01-07T00:00:00Z");
}

#[tokio::test]
async fn cursor_pagination_follows_tokens() {
    let base = serve_api().await;
    let config = json!({"manifest": manifest(&base), "api_key": "k-123"});
    let messages = read(&config, &[("events", "full_refresh")], None).await;
    let data = records(&messages);
    assert_eq!(data.len(), 3);
    assert_eq!(data[2]["id"], "e3");
    // No cursor field → no state emitted.
    assert!(messages.iter().all(|m| m.state.is_none()));
}

#[tokio::test]
async fn incremental_resume_skips_synced_records() {
    let base = serve_api().await;
    let config = json!({"manifest": manifest(&base), "api_key": "k-123"});

    let first = read(&config, &[("users", "incremental")], None).await;
    assert_eq!(records(&first).len(), 7);
    let state = json!([
        serde_json::to_value(first.iter().find_map(|m| m.state.as_ref()).unwrap()).unwrap()
    ]);

    // Resume: everything is at-or-before the cursor → nothing re-emitted.
    let second = read(&config, &[("users", "incremental")], Some(&state)).await;
    assert_eq!(records(&second).len(), 0);

    // Same state under full_refresh re-reads everything.
    let full = read(&config, &[("users", "full_refresh")], Some(&state)).await;
    assert_eq!(records(&full).len(), 7);
}

/// The compiled binary over the real wire protocol, launched exactly as the
/// platform launches it (`exec:` scheme).
#[tokio::test]
async fn binary_end_to_end() {
    use gauss_connector_runtime::{ConnectorRunner, ProcessLauncher};

    let base = serve_api().await;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    std::fs::write(
        &config_path,
        serde_json::to_vec(&json!({"manifest": manifest(&base), "api_key": "k-123"})).unwrap(),
    )
    .unwrap();

    let runner = ConnectorRunner::new(ProcessLauncher::new(env!(
        "CARGO_BIN_EXE_gauss-declarative"
    )));

    let spec = runner.spec().await.expect("spec");
    assert!(spec.connection_specification["properties"]["manifest"].is_object());

    let status = runner.check(&config_path).await.expect("check");
    assert_eq!(
        status.status,
        gauss_cdk::protocol::ConnectionStatus::Succeeded
    );

    let discovered = runner.discover(&config_path).await.expect("discover");
    assert_eq!(discovered.streams.len(), 2);

    let catalog_path = dir.path().join("catalog.json");
    std::fs::write(
        &catalog_path,
        serde_json::to_vec(&catalog(&[("users", "incremental")])).unwrap(),
    )
    .unwrap();
    let mut record_count = 0;
    let summary = runner
        .read(&config_path, &catalog_path, None, |event| {
            if let gauss_connector_runtime::ReadEvent::Message(msg) = event {
                if msg.record.is_some() {
                    record_count += 1;
                }
            }
        })
        .await
        .expect("read");
    assert_eq!(record_count, 7);
    assert_eq!(summary.records, 7);
    assert!(summary.last_state.is_some());
}

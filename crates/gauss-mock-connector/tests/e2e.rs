//! End-to-end: drive the compiled mock connector through
//! `gauss-connector-runtime` exactly as the platform will drive real
//! connectors — spec, check, discover, full read, and resumed read.

use gauss_connector_runtime::{ConnectorRunner, ProcessLauncher, ReadEvent};
use gauss_protocol::*;
use serde_json::json;
use std::path::PathBuf;

fn runner() -> ConnectorRunner {
    ConnectorRunner::new(ProcessLauncher::new(env!(
        "CARGO_BIN_EXE_gauss-mock-connector"
    )))
}

fn write_json(dir: &tempfile::TempDir, name: &str, value: &serde_json::Value) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn configured_catalog() -> serde_json::Value {
    json!({
        "streams": [{
            "stream": {"name": "users", "json_schema": {"type": "object"}},
            "sync_mode": "incremental",
            "cursor_field": ["id"],
            "destination_sync_mode": "append"
        }]
    })
}

#[tokio::test]
async fn spec_roundtrip() {
    let spec = runner().spec().await.expect("spec must succeed");
    assert_eq!(spec.supports_incremental, Some(true));
    assert!(spec.connection_specification["properties"]["record_count"].is_object());
}

#[tokio::test]
async fn check_succeeds_and_fails() {
    let dir = tempfile::tempdir().unwrap();

    let good = write_json(&dir, "good.json", &json!({"record_count": 1}));
    let status = runner().check(&good).await.unwrap();
    assert_eq!(status.status, ConnectionStatus::Succeeded);

    let bad = write_json(&dir, "bad.json", &json!({"fail_check": true}));
    let status = runner().check(&bad).await.unwrap();
    assert_eq!(status.status, ConnectionStatus::Failed);
}

#[tokio::test]
async fn discover_lists_users_stream() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_json(&dir, "config.json", &json!({}));
    let catalog = runner().discover(&config).await.unwrap();
    assert_eq!(catalog.streams.len(), 1);
    assert_eq!(catalog.streams[0].name, "users");
    assert_eq!(
        catalog.streams[0].source_defined_primary_key,
        Some(vec![vec!["id".to_string()]])
    );
}

#[tokio::test]
async fn read_emits_records_and_checkpoints() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_json(&dir, "config.json", &json!({"record_count": 12}));
    let catalog = write_json(&dir, "catalog.json", &configured_catalog());

    let mut statuses = vec![];
    let summary = runner()
        .read(&config, &catalog, None, |event| {
            if let ReadEvent::Message(msg) = event {
                if let Some(trace) = &msg.trace {
                    if let Some(s) = &trace.stream_status {
                        statuses.push(s.status);
                    }
                }
            }
        })
        .await
        .unwrap();

    assert_eq!(summary.records, 12);
    // Checkpoints at ids 5, 10, 12.
    assert_eq!(summary.state_messages, 3);
    assert_eq!(
        statuses,
        vec![StreamStatus::Started, StreamStatus::Complete]
    );

    let last = summary.last_state.unwrap();
    let stream_state = last.stream.unwrap().stream_state.unwrap();
    assert_eq!(stream_state["cursor"], 12);
}

#[tokio::test]
async fn read_resumes_from_state() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_json(&dir, "config.json", &json!({"record_count": 10}));
    let catalog = write_json(&dir, "catalog.json", &configured_catalog());
    let state = write_json(
        &dir,
        "state.json",
        &json!([{
            "type": "STREAM",
            "stream": {
                "stream_descriptor": {"name": "users"},
                "stream_state": {"cursor": 7}
            }
        }]),
    );

    let mut first_id: Option<i64> = None;
    let summary = runner()
        .read(&config, &catalog, Some(&state), |event| {
            if let ReadEvent::Message(msg) = event {
                if let Some(record) = &msg.record {
                    first_id.get_or_insert(record.data["id"].as_i64().unwrap());
                }
            }
        })
        .await
        .unwrap();

    // Resumed after cursor 7 → records 8, 9, 10.
    assert_eq!(summary.records, 3);
    assert_eq!(first_id, Some(8));
}

#[tokio::test]
async fn unselected_stream_yields_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_json(&dir, "config.json", &json!({"record_count": 5}));
    let catalog = write_json(&dir, "catalog.json", &json!({"streams": []}));

    let summary = runner()
        .read(&config, &catalog, None, |_| {})
        .await
        .unwrap();
    assert_eq!(summary.records, 0);
}

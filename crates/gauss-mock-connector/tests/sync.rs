//! End-to-end replication tests: the mock connector as *both* source and
//! destination, piped through `gauss_sync::run_sync` exactly as the
//! orchestrator runs production syncs.

use std::sync::{Arc, Mutex};

use gauss_connector_runtime::ProcessLauncher;
use gauss_sync::{run_sync, SyncError, SyncOptions, SyncRequest};
use serde_json::{json, Value};
use tokio::sync::watch;

fn launcher() -> ProcessLauncher {
    ProcessLauncher::new(env!("CARGO_BIN_EXE_gauss-mock-connector"))
}

fn catalog() -> Value {
    json!({
        "streams": [{
            "stream": {"name": "users", "json_schema": {"type": "object"}},
            "sync_mode": "incremental",
            "cursor_field": ["id"],
            "destination_sync_mode": "append"
        }]
    })
}

/// Runs a sync and returns (summary, checkpoints seen, destination file).
async fn sync(
    source_config: Value,
    state: Option<Value>,
) -> (
    Result<gauss_sync::SyncSummary, SyncError>,
    Vec<Value>,
    tempfile::NamedTempFile,
) {
    let out_file = tempfile::NamedTempFile::new().unwrap();
    let request = SyncRequest {
        source_config,
        destination_config: json!({"out_path": out_file.path()}),
        catalog: catalog(),
        state,
    };
    let checkpoints: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(vec![]));
    let seen = checkpoints.clone();
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    let result = run_sync(
        &launcher(),
        &launcher(),
        &request,
        &SyncOptions::default(),
        cancel_rx,
        move |state| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(state);
                Ok(())
            }
        },
    )
    .await;

    let checkpoints = checkpoints.lock().unwrap().clone();
    (result, checkpoints, out_file)
}

#[tokio::test]
async fn full_sync_delivers_records_and_checkpoints() {
    let (result, checkpoints, out_file) = sync(json!({"record_count": 12}), None).await;
    let summary = result.expect("sync must succeed");

    assert_eq!(summary.records_synced, 12);
    // Source checkpoints at ids 5, 10, 12 — all acked by the destination.
    assert_eq!(summary.committed_states, 3);
    assert_eq!(checkpoints.len(), 3);
    assert_eq!(
        summary.stream_statuses.get("users").map(String::as_str),
        Some("complete")
    );

    // Destination wrote every record.
    let written = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(written.lines().count(), 12);
    assert!(written.contains("user-12@example.com"));

    // Final state resumes from the end.
    let final_state = summary.final_state.expect("final state");
    assert_eq!(final_state[0]["stream"]["stream_state"]["cursor"], 12);
}

#[tokio::test]
async fn sync_resumes_from_state() {
    let state = json!([{
        "type": "STREAM",
        "stream": {
            "stream_descriptor": {"name": "users"},
            "stream_state": {"cursor": 7}
        }
    }]);
    let (result, _checkpoints, out_file) = sync(json!({"record_count": 10}), Some(state)).await;
    let summary = result.expect("sync must succeed");

    // Only records 8, 9, 10 flow.
    assert_eq!(summary.records_synced, 3);
    let written = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(written.lines().count(), 3);
    assert!(written.contains("\"id\":8"));
    assert!(!written.contains("\"id\":7"));

    let final_state = summary.final_state.unwrap();
    assert_eq!(final_state[0]["stream"]["stream_state"]["cursor"], 10);
}

#[tokio::test]
async fn cancellation_stops_a_running_sync() {
    let out_file = tempfile::NamedTempFile::new().unwrap();
    let request = SyncRequest {
        // Slow source: 1000 records at 5ms each ≈ 5s without cancellation.
        source_config: json!({"record_count": 1000, "emit_delay_ms": 5}),
        destination_config: json!({"out_path": out_file.path()}),
        catalog: catalog(),
        state: None,
    };
    let (cancel_tx, cancel_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = cancel_tx.send(true);
    });

    let started = std::time::Instant::now();
    let result = run_sync(
        &launcher(),
        &launcher(),
        &request,
        &SyncOptions::default(),
        cancel_rx,
        |_| async { Ok(()) },
    )
    .await;

    assert!(matches!(result, Err(SyncError::Cancelled)), "{result:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[tokio::test]
async fn destination_spawn_failure_fails_the_sync() {
    let request = SyncRequest {
        source_config: json!({"record_count": 1}),
        destination_config: json!({}),
        catalog: catalog(),
        state: None,
    };
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let bad_destination = ProcessLauncher::new("/nonexistent/connector");

    let result = run_sync(
        &launcher(),
        &bad_destination,
        &request,
        &SyncOptions::default(),
        cancel_rx,
        |_| async { Ok(()) },
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn checkpoint_failure_aborts_the_sync() {
    let out_file = tempfile::NamedTempFile::new().unwrap();
    let request = SyncRequest {
        source_config: json!({"record_count": 10}),
        destination_config: json!({"out_path": out_file.path()}),
        catalog: catalog(),
        state: None,
    };
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    let result = run_sync(
        &launcher(),
        &launcher(),
        &request,
        &SyncOptions::default(),
        cancel_rx,
        |_| async { Err("state store unavailable".to_string()) },
    )
    .await;
    assert!(
        matches!(&result, Err(SyncError::Checkpoint(msg)) if msg.contains("unavailable")),
        "{result:?}"
    );
}

//! Orchestrator integration tests: real Postgres queue + the mock connector
//! binary as both source and destination (via the `exec:` launcher scheme).
//!
//! Requires `DATABASE_URL` (skips otherwise) and the workspace-built
//! `gauss-mock-connector` binary (`cargo test --workspace` builds it).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gauss_orchestrator::{next_due, Orchestrator, WorkerOptions};
use gauss_store::{ActorType, NewActor, NewConnection, NewDefinition, Store};
use serde_json::json;
use uuid::Uuid;

fn mock_connector_bin() -> Option<PathBuf> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gauss-mock-connector");
    path.canonicalize().ok().filter(|p| p.exists())
}

async fn test_store() -> Option<Store> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping");
        return None;
    };
    let admin = sqlx::PgPool::connect(&url).await.expect("admin connect");
    let name = format!("gauss_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
        .execute(&admin)
        .await
        .expect("create test database");
    let (base, _) = url.rsplit_once('/').unwrap();
    Some(Store::connect(&format!("{base}/{name}")).await.unwrap())
}

fn orchestrator(store: &Store) -> Orchestrator {
    Orchestrator::new(
        store.clone(),
        Arc::new(store.secrets_backend()),
        WorkerOptions {
            poll_interval: Duration::from_millis(100),
            max_attempts: 2,
            retry_backoff: Duration::from_secs(0),
            ..WorkerOptions::default()
        },
    )
}

struct Fixture {
    connection_id: Uuid,
    out_path: PathBuf,
    _out_dir: tempfile::TempDir,
}

/// Workspace + exec-scheme definitions + actors + an incremental connection.
async fn fixture(store: &Store, source_repo: &str, record_count: u64) -> Fixture {
    let out_dir = tempfile::tempdir().unwrap();
    let out_path = out_dir.path().join("out.ndjson");

    let workspace = store.workspaces().create("e2e").await.unwrap();
    let source_def = store
        .definitions()
        .upsert(&NewDefinition {
            id: Uuid::new_v4(),
            actor_type: ActorType::Source,
            name: "mock source".into(),
            docker_repository: source_repo.into(),
            docker_image_tag: "dev".into(),
            documentation_url: None,
            spec: None,
        })
        .await
        .unwrap();
    let destination_def = store
        .definitions()
        .upsert(&NewDefinition {
            id: Uuid::new_v4(),
            actor_type: ActorType::Destination,
            name: "mock destination".into(),
            docker_repository: source_repo.into(),
            docker_image_tag: "dev".into(),
            documentation_url: None,
            spec: None,
        })
        .await
        .unwrap();

    let source = store
        .actors()
        .create(&NewActor {
            workspace_id: workspace.id,
            definition_id: source_def.id,
            actor_type: ActorType::Source,
            name: "src".into(),
            configuration: json!({"record_count": record_count}),
        })
        .await
        .unwrap();
    let destination = store
        .actors()
        .create(&NewActor {
            workspace_id: workspace.id,
            definition_id: destination_def.id,
            actor_type: ActorType::Destination,
            name: "dst".into(),
            configuration: json!({"out_path": out_path}),
        })
        .await
        .unwrap();

    let connection = store
        .connections()
        .create(&NewConnection {
            workspace_id: workspace.id,
            source_id: source.id,
            destination_id: destination.id,
            name: "users sync".into(),
            catalog: json!({
                "streams": [{
                    "stream": {"name": "users", "json_schema": {"type": "object"}},
                    "sync_mode": "incremental",
                    "cursor_field": ["id"],
                    "destination_sync_mode": "append"
                }]
            }),
            schedule: None,
            notifications: None,
        })
        .await
        .unwrap();

    Fixture {
        connection_id: connection.id,
        out_path,
        _out_dir: out_dir,
    }
}

#[tokio::test]
async fn job_executes_sync_and_persists_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let Some(bin) = mock_connector_bin() else {
        eprintln!("mock connector binary not built; skipping");
        return;
    };
    let repo = format!("exec:{}", bin.display());
    let fx = fixture(&store, &repo, 12).await;
    let orch = orchestrator(&store);

    let job = store.jobs().create(fx.connection_id, "sync").await.unwrap();
    let outcome = orch.run_pending_once().await.unwrap().expect("one job due");

    assert_eq!(outcome.job_id, job.id);
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(outcome.records_synced, 12);

    // Job + attempt bookkeeping.
    let job = store.jobs().get(job.id).await.unwrap();
    assert_eq!(job.status, "succeeded");
    assert!(job.completed_at.is_some());
    let attempts = store.jobs().list_attempts(job.id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, "succeeded");
    assert_eq!(attempts[0].records_synced, Some(12));

    // Connection state checkpointed for the next run.
    let state = store
        .connection_states()
        .get(fx.connection_id)
        .await
        .unwrap()
        .expect("state persisted");
    assert_eq!(state[0]["stream"]["stream_state"]["cursor"], 12);

    // Destination received everything.
    let written = std::fs::read_to_string(&fx.out_path).unwrap();
    assert_eq!(written.lines().count(), 12);

    // Queue is drained.
    assert!(orch.run_pending_once().await.unwrap().is_none());
}

#[tokio::test]
async fn second_job_resumes_incrementally() {
    let Some(store) = test_store().await else {
        return;
    };
    let Some(bin) = mock_connector_bin() else {
        eprintln!("mock connector binary not built; skipping");
        return;
    };
    let repo = format!("exec:{}", bin.display());
    let fx = fixture(&store, &repo, 10).await;
    let orch = orchestrator(&store);

    store.jobs().create(fx.connection_id, "sync").await.unwrap();
    let first = orch.run_pending_once().await.unwrap().unwrap();
    assert_eq!(first.records_synced, 10);

    // Same record_count: nothing new to read on the second run.
    store.jobs().create(fx.connection_id, "sync").await.unwrap();
    let second = orch.run_pending_once().await.unwrap().unwrap();
    assert_eq!(second.status, "succeeded");
    assert_eq!(
        second.records_synced, 0,
        "incremental resume must skip synced records"
    );

    let written = std::fs::read_to_string(&fx.out_path).unwrap();
    assert_eq!(written.lines().count(), 10);
}

#[tokio::test]
async fn failing_job_retries_then_fails() {
    let Some(store) = test_store().await else {
        return;
    };
    // Connectors that cannot spawn: every attempt fails.
    let fx = fixture(&store, "exec:/nonexistent/connector", 5).await;
    let orch = orchestrator(&store);

    let job = store.jobs().create(fx.connection_id, "sync").await.unwrap();

    // Attempt 1: fails and is rescheduled (backoff 0 in tests).
    let outcome = orch.run_pending_once().await.unwrap().unwrap();
    assert_eq!(outcome.status, "pending");
    assert_eq!(store.jobs().get(job.id).await.unwrap().status, "pending");

    // Attempt 2 (== max_attempts): fails terminally.
    let outcome = orch.run_pending_once().await.unwrap().unwrap();
    assert_eq!(outcome.status, "failed");
    let job = store.jobs().get(job.id).await.unwrap();
    assert_eq!(job.status, "failed");
    let attempts = store.jobs().list_attempts(job.id).await.unwrap();
    assert_eq!(attempts.len(), 2);
    assert!(attempts.iter().all(|a| a.status == "failed"));
}

#[tokio::test]
async fn duplicate_and_cancel_semantics() {
    let Some(store) = test_store().await else {
        return;
    };
    let Some(bin) = mock_connector_bin() else {
        eprintln!("mock connector binary not built; skipping");
        return;
    };
    let fx = fixture(&store, &format!("exec:{}", bin.display()), 5).await;

    let job = store.jobs().create(fx.connection_id, "sync").await.unwrap();
    // Second enqueue while one is pending → unique-index conflict.
    assert!(matches!(
        store.jobs().create(fx.connection_id, "sync").await,
        Err(gauss_store::StoreError::Conflict(_))
    ));

    // Cancelling a pending job terminates it immediately.
    let cancelled = store.jobs().cancel(job.id).await.unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert!(cancelled.cancel_requested);

    // Cancelling a terminal job conflicts.
    assert!(store.jobs().cancel(job.id).await.is_err());

    // The queue no longer offers it.
    let orch = orchestrator(&store);
    assert!(orch.run_pending_once().await.unwrap().is_none());
}

#[tokio::test]
async fn scheduler_enqueues_due_connections_once() {
    let Some(store) = test_store().await else {
        return;
    };
    let Some(bin) = mock_connector_bin() else {
        eprintln!("mock connector binary not built; skipping");
        return;
    };
    let fx = fixture(&store, &format!("exec:{}", bin.display()), 3).await;
    store
        .connections()
        .update(
            fx.connection_id,
            &gauss_store::ConnectionPatch {
                schedule: Some(json!({"intervalMinutes": 0})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let orch = orchestrator(&store);

    // Never ran → due now; enqueues exactly one job.
    assert_eq!(orch.schedule_due_once().await.unwrap(), 1);
    // A pending job exists → not schedulable again.
    assert_eq!(orch.schedule_due_once().await.unwrap(), 0);

    // Run it, then interval 0 makes it immediately due again.
    let outcome = orch.run_pending_once().await.unwrap().unwrap();
    assert_eq!(outcome.status, "succeeded");
    assert_eq!(orch.schedule_due_once().await.unwrap(), 1);
}

#[test]
fn schedule_evaluation() {
    use chrono::{TimeZone, Utc};
    let last = Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();

    // Interval: last + N minutes.
    let due = next_due(&json!({"intervalMinutes": 60}), Some(last)).unwrap();
    assert_eq!(due, last + chrono::Duration::minutes(60));

    // Standard 5-field cron (hourly on the hour), next fire after last.
    let due = next_due(&json!({"cron": "0 * * * *"}), Some(last)).unwrap();
    assert_eq!(due, Utc.with_ymd_and_hms(2026, 6, 10, 13, 0, 0).unwrap());

    // 6-field cron with seconds also accepted.
    assert!(next_due(&json!({"cron": "0 0 * * * *"}), Some(last)).is_ok());

    // Garbage rejected.
    assert!(next_due(&json!({"cron": "not a cron"}), Some(last)).is_err());
    assert!(next_due(&json!({}), Some(last)).is_err());
}

#[tokio::test]
async fn webhook_fires_on_terminal_job() {
    let Some(store) = test_store().await else {
        return;
    };
    let Some(bin) = mock_connector_bin() else {
        eprintln!("mock connector binary not built; skipping");
        return;
    };

    // Local webhook receiver forwarding payloads over a channel.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
    let receiver = {
        use axum::routing::post;
        let app = axum::Router::new().route(
            "/hook",
            post(move |axum::Json(payload): axum::Json<serde_json::Value>| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(payload);
                    "ok"
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/hook")
    };

    let fx = fixture(&store, &format!("exec:{}", bin.display()), 4).await;
    store
        .connections()
        .update(
            fx.connection_id,
            &gauss_store::ConnectionPatch {
                notifications: Some(json!({"webhookUrl": receiver})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let orch = orchestrator(&store);
    store.jobs().create(fx.connection_id, "sync").await.unwrap();
    let outcome = orch.run_pending_once().await.unwrap().unwrap();
    assert_eq!(outcome.status, "succeeded");

    let payload = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("webhook delivered")
        .unwrap();
    assert_eq!(payload["event"], "job.completed");
    assert_eq!(payload["status"], "succeeded");
    assert_eq!(payload["recordsSynced"], 4);
    assert_eq!(
        payload["connectionId"].as_str().unwrap(),
        fx.connection_id.to_string()
    );
}

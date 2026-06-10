//! Executes one claimed job: attempt bookkeeping, heartbeats, cancellation
//! watching, sync execution, retry/backoff decisions.

use std::collections::BTreeMap;
use std::sync::Arc;

use gauss_connector_runtime::resolve_launcher;
use gauss_store::{Actor, ActorType, Job};
use gauss_sync::{state_key, SyncError, SyncOptions, SyncRequest};
use serde_json::Value;
use tokio::sync::{watch, Mutex};

use crate::{Orchestrator, OrchestratorError};

#[derive(Debug)]
pub struct JobOutcome {
    pub job_id: i64,
    /// Terminal or requeued job status: succeeded | failed | cancelled | pending.
    pub status: String,
    pub records_synced: u64,
    pub attempt_number: i32,
}

impl Orchestrator {
    pub(crate) async fn execute(&self, job: Job) -> JobOutcome {
        let attempt = match self.store.jobs().create_attempt(job.id).await {
            Ok(attempt) => attempt,
            Err(err) => {
                tracing::error!(job = job.id, %err, "creating attempt failed");
                return self.finish(&job, "failed", 0, 0).await;
            }
        };

        // A reaped job can exhaust its attempts before the worker ever sees
        // the failure path; guard here too.
        if attempt.attempt_number > self.options.max_attempts {
            let _ = self
                .store
                .jobs()
                .finish_attempt(attempt.id, "failed", None, None)
                .await;
            return self.finish(&job, "failed", 0, attempt.attempt_number).await;
        }

        // Heartbeat while the sync runs.
        let heartbeat = {
            let store = self.store.clone();
            let interval = self.options.heartbeat_interval;
            let attempt_id = attempt.id;
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(interval).await;
                    if store.jobs().heartbeat(attempt_id).await.is_err() {
                        break;
                    }
                }
            })
        };

        // Translate DB-level cancel requests into the sync's watch channel.
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let cancel_watcher = {
            let store = self.store.clone();
            let job_id = job.id;
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if store.jobs().cancel_requested(job_id).await.unwrap_or(false) {
                        let _ = cancel_tx.send(true);
                        break;
                    }
                }
            })
        };

        let result = self.run_sync_job(&job, cancel_rx).await;
        heartbeat.abort();
        cancel_watcher.abort();

        let outcome = match result {
            Ok((summary, final_state)) => {
                let _ = self
                    .store
                    .jobs()
                    .finish_attempt(
                        attempt.id,
                        "succeeded",
                        Some(summary.records_synced as i64),
                        final_state.as_ref(),
                    )
                    .await;
                self.finish(
                    &job,
                    "succeeded",
                    summary.records_synced,
                    attempt.attempt_number,
                )
                .await
            }
            Err(OrchestratorError::Sync(SyncError::Cancelled)) => {
                let _ = self
                    .store
                    .jobs()
                    .finish_attempt(attempt.id, "failed", None, None)
                    .await;
                self.finish(&job, "cancelled", 0, attempt.attempt_number)
                    .await
            }
            Err(err) => {
                tracing::warn!(job = job.id, attempt = attempt.attempt_number, %err, "attempt failed");
                let _ = self
                    .store
                    .jobs()
                    .finish_attempt(attempt.id, "failed", None, None)
                    .await;
                if attempt.attempt_number < self.options.max_attempts {
                    let backoff = self.options.retry_backoff.as_secs() as i64
                        * 2_i64.pow((attempt.attempt_number - 1).max(0) as u32);
                    match self.store.jobs().reschedule(job.id, backoff).await {
                        Ok(_) => JobOutcome {
                            job_id: job.id,
                            status: "pending".to_string(),
                            records_synced: 0,
                            attempt_number: attempt.attempt_number,
                        },
                        Err(_) => self.finish(&job, "failed", 0, attempt.attempt_number).await,
                    }
                } else {
                    self.finish(&job, "failed", 0, attempt.attempt_number).await
                }
            }
        };
        if outcome.status != "pending" {
            self.notify(&job, &outcome).await;
        }
        outcome
    }

    /// Best-effort webhook on terminal jobs, when the connection configures
    /// `notifications.webhookUrl`.
    async fn notify(&self, job: &Job, outcome: &JobOutcome) {
        let Ok(connection) = self.store.connections().get(job.connection_id).await else {
            return;
        };
        let Some(url) = connection
            .notifications
            .as_ref()
            .and_then(|n| n.0.get("webhookUrl"))
            .and_then(Value::as_str)
        else {
            return;
        };
        let payload = serde_json::json!({
            "event": "job.completed",
            "jobId": outcome.job_id,
            "connectionId": job.connection_id,
            "connectionName": connection.name,
            "status": outcome.status,
            "recordsSynced": outcome.records_synced,
            "attempt": outcome.attempt_number,
        });
        let result = reqwest::Client::new()
            .post(url)
            .timeout(std::time::Duration::from_secs(10))
            .json(&payload)
            .send()
            .await;
        match result {
            Ok(response) if !response.status().is_success() => {
                tracing::warn!(job = job.id, status = %response.status(), "webhook rejected");
            }
            Err(err) => tracing::warn!(job = job.id, %err, "webhook delivery failed"),
            _ => {}
        }
    }

    async fn finish(
        &self,
        job: &Job,
        status: &str,
        records_synced: u64,
        attempt_number: i32,
    ) -> JobOutcome {
        if let Err(err) = self.store.jobs().finish(job.id, status).await {
            tracing::error!(job = job.id, %err, "finishing job failed");
        }
        JobOutcome {
            job_id: job.id,
            status: status.to_string(),
            records_synced,
            attempt_number,
        }
    }

    /// Load everything the sync needs, hydrate secrets, and run it with
    /// mid-flight checkpoint persistence.
    async fn run_sync_job(
        &self,
        job: &Job,
        cancel: watch::Receiver<bool>,
    ) -> Result<(gauss_sync::SyncSummary, Option<Value>), OrchestratorError> {
        let connection = self.store.connections().get(job.connection_id).await?;
        let (source, source_def) = self
            .load_actor(connection.source_id, ActorType::Source)
            .await?;
        let (destination, destination_def) = self
            .load_actor(connection.destination_id, ActorType::Destination)
            .await?;

        let source_config =
            gauss_secrets::hydrate_config(&source.configuration.0, self.secrets.as_ref()).await?;
        let destination_config =
            gauss_secrets::hydrate_config(&destination.configuration.0, self.secrets.as_ref())
                .await?;
        let state = self.store.connection_states().get(connection.id).await?;

        let request = SyncRequest {
            source_config,
            destination_config,
            catalog: connection.catalog.0.clone(),
            state,
        };

        // Persist each destination-acked checkpoint immediately, merged into
        // the connection's per-stream state map.
        let state_map: Arc<Mutex<BTreeMap<String, Value>>> =
            Arc::new(Mutex::new(match &request.state {
                Some(Value::Array(messages)) => {
                    messages.iter().map(|m| (state_key(m), m.clone())).collect()
                }
                _ => BTreeMap::new(),
            }));
        let checkpoint_store = self.store.clone();
        let checkpoint_map = state_map.clone();
        let connection_id = connection.id;
        let on_checkpoint = move |state: Value| {
            let store = checkpoint_store.clone();
            let map = checkpoint_map.clone();
            async move {
                let mut map = map.lock().await;
                map.insert(state_key(&state), state);
                let merged = Value::Array(map.values().cloned().collect());
                store
                    .connection_states()
                    .set(connection_id, &merged)
                    .await
                    .map_err(|err| err.to_string())
            }
        };

        let source_launcher =
            resolve_launcher(&source_def.docker_repository, &source_def.docker_image_tag);
        let destination_launcher = resolve_launcher(
            &destination_def.docker_repository,
            &destination_def.docker_image_tag,
        );

        let summary = gauss_sync::run_sync(
            source_launcher.as_ref(),
            destination_launcher.as_ref(),
            &request,
            &SyncOptions {
                idle_timeout: self.options.idle_timeout,
            },
            cancel,
            on_checkpoint,
        )
        .await?;

        let final_state = {
            let map = state_map.lock().await;
            if map.is_empty() {
                None
            } else {
                Some(Value::Array(map.values().cloned().collect()))
            }
        };
        Ok((summary, final_state))
    }

    async fn load_actor(
        &self,
        id: uuid::Uuid,
        actor_type: ActorType,
    ) -> Result<(Actor, gauss_store::ActorDefinition), OrchestratorError> {
        let actor = self.store.actors().get(id, actor_type).await?;
        let definition = self.store.definitions().get(actor.definition_id).await?;
        Ok((actor, definition))
    }
}

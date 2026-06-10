//! Postgres-backed job orchestration — gaussdataflow's replacement for
//! Airbyte's Temporal dependency.
//!
//! The `jobs` table *is* the queue: workers claim with `FOR UPDATE SKIP
//! LOCKED` ([`gauss_store::repo::jobs`]), so any number of orchestrator
//! processes can run against the same database with no coordinator.
//!
//! Each claimed sync job: load connection + actors, hydrate configs from the
//! secrets backend, resolve launchers, and run [`gauss_sync::run_sync`].
//! Destination-acked checkpoints are persisted to `connection_states`
//! mid-flight, so a crash never loses committed progress. Failures retry
//! with exponential backoff up to `max_attempts`; a heartbeat task marks the
//! attempt alive and stale jobs from crashed workers are reaped back into
//! the queue.

mod executor;
mod scheduler;

pub use executor::JobOutcome;
pub use scheduler::next_due;

use std::sync::Arc;
use std::time::Duration;

use gauss_secrets::SecretsBackend;
use gauss_store::Store;
use tokio::sync::watch;

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("store error: {0}")]
    Store(#[from] gauss_store::StoreError),
    #[error("secrets error: {0}")]
    Secrets(#[from] gauss_secrets::SecretsError),
    #[error("sync error: {0}")]
    Sync(#[from] gauss_sync::SyncError),
    #[error("invalid schedule: {0}")]
    Schedule(String),
}

#[derive(Clone)]
pub struct WorkerOptions {
    pub poll_interval: Duration,
    pub max_attempts: i32,
    /// First retry delay; doubles per attempt.
    pub retry_backoff: Duration,
    pub heartbeat_interval: Duration,
    /// Running attempts without a heartbeat for this long are reaped.
    pub stale_after: Duration,
    pub idle_timeout: Duration,
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            max_attempts: 3,
            retry_backoff: Duration::from_secs(10),
            heartbeat_interval: Duration::from_secs(10),
            stale_after: Duration::from_secs(120),
            idle_timeout: Duration::from_secs(300),
        }
    }
}

pub struct Orchestrator {
    pub(crate) store: Store,
    pub(crate) secrets: Arc<dyn SecretsBackend>,
    pub(crate) options: WorkerOptions,
}

impl Orchestrator {
    pub fn new(store: Store, secrets: Arc<dyn SecretsBackend>, options: WorkerOptions) -> Self {
        Self {
            store,
            secrets,
            options,
        }
    }

    /// Claim and fully execute one due job. `Ok(None)` when the queue is
    /// empty — the unit the worker loop (and tests) drive.
    pub async fn run_pending_once(&self) -> Result<Option<JobOutcome>, OrchestratorError> {
        let Some(job) = self.store.jobs().claim_next().await? else {
            return Ok(None);
        };
        Ok(Some(self.execute(job).await))
    }

    /// Enqueue jobs for scheduled connections that are due. Returns how many
    /// jobs were created.
    pub async fn schedule_due_once(&self) -> Result<usize, OrchestratorError> {
        scheduler::schedule_due(&self.store).await
    }

    /// The worker loop: reap stale jobs, enqueue due schedules, drain the
    /// queue, sleep, repeat — until `shutdown` flips to true.
    pub async fn run(self: Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        tracing::info!("orchestrator worker started");
        loop {
            if let Err(err) = self
                .store
                .jobs()
                .reap_stale(self.options.stale_after.as_secs() as i64)
                .await
            {
                tracing::error!(%err, "reaping stale jobs failed");
            }
            if let Err(err) = self.schedule_due_once().await {
                tracing::error!(%err, "scheduling failed");
            }
            loop {
                match self.run_pending_once().await {
                    Ok(Some(outcome)) => {
                        tracing::info!(job = outcome.job_id, status = %outcome.status, "job finished");
                    }
                    Ok(None) => break,
                    Err(err) => {
                        tracing::error!(%err, "job execution errored");
                        break;
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(self.options.poll_interval) => {}
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("orchestrator worker stopping");
                        return;
                    }
                }
            }
        }
    }
}

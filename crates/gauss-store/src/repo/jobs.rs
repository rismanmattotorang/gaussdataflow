use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::{Attempt, Job, JobOverview, PlatformStats};

pub struct JobRepo<'a> {
    pub(crate) pool: &'a PgPool,
}

const COLUMNS: &str = "id, connection_id, job_type, status, scheduled_at, started_at, \
                       completed_at, cancel_requested, created_at, updated_at";
const ATTEMPT_COLUMNS: &str = "id, job_id, attempt_number, status, records_synced, state, \
                               created_at, ended_at, last_heartbeat_at";

impl JobRepo<'_> {
    /// Enqueue a job. The partial unique index guarantees at most one
    /// pending/running job per connection; violations surface as `Conflict`.
    pub async fn create(&self, connection_id: Uuid, job_type: &str) -> Result<Job, StoreError> {
        sqlx::query_as::<_, Job>(&format!(
            "INSERT INTO jobs (connection_id, job_type) VALUES ($1, $2) RETURNING {COLUMNS}"
        ))
        .bind(connection_id)
        .bind(job_type)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "job"))
    }

    /// Claim the next due pending job (`FOR UPDATE SKIP LOCKED`): safe for
    /// many concurrent workers, no coordinator needed.
    pub async fn claim_next(&self) -> Result<Option<Job>, StoreError> {
        Ok(sqlx::query_as::<_, Job>(&format!(
            "UPDATE jobs SET status = 'running',
                             started_at = COALESCE(started_at, now()),
                             updated_at = now()
             WHERE id = (
                 SELECT id FROM jobs
                 WHERE status = 'pending' AND scheduled_at <= now()
                 ORDER BY scheduled_at, id
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING {COLUMNS}"
        ))
        .fetch_optional(self.pool)
        .await?)
    }

    pub async fn get(&self, id: i64) -> Result<Job, StoreError> {
        sqlx::query_as::<_, Job>(&format!("SELECT {COLUMNS} FROM jobs WHERE id = $1"))
            .bind(id)
            .fetch_optional(self.pool)
            .await?
            .ok_or(StoreError::NotFound("job"))
    }

    pub async fn list(&self, connection_id: Uuid) -> Result<Vec<Job>, StoreError> {
        Ok(sqlx::query_as::<_, Job>(&format!(
            "SELECT {COLUMNS} FROM jobs WHERE connection_id = $1
             ORDER BY created_at DESC LIMIT 100"
        ))
        .bind(connection_id)
        .fetch_all(self.pool)
        .await?)
    }

    /// Recent jobs across every connection, newest first, enriched with the
    /// connection name and latest attempt record count. `workspace_id`
    /// narrows the view to one workspace.
    pub async fn list_recent(
        &self,
        workspace_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<JobOverview>, StoreError> {
        Ok(sqlx::query_as::<_, JobOverview>(
            "SELECT j.id, j.connection_id, c.name AS connection_name, c.workspace_id,
                    j.job_type, j.status, j.scheduled_at, j.started_at, j.completed_at,
                    j.cancel_requested, j.created_at, j.updated_at,
                    (SELECT a.records_synced FROM attempts a
                     WHERE a.job_id = j.id
                     ORDER BY a.attempt_number DESC LIMIT 1) AS records_synced
             FROM jobs j
             JOIN connections c ON c.id = j.connection_id
             WHERE $1::uuid IS NULL OR c.workspace_id = $1
             ORDER BY j.created_at DESC
             LIMIT $2",
        )
        .bind(workspace_id)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.pool)
        .await?)
    }

    /// Aggregate fleet health in one round trip, optionally scoped to a
    /// workspace.
    pub async fn platform_stats(
        &self,
        workspace_id: Option<Uuid>,
    ) -> Result<PlatformStats, StoreError> {
        Ok(sqlx::query_as::<_, PlatformStats>(
            "SELECT
                (SELECT count(*) FROM workspaces w
                 WHERE $1::uuid IS NULL OR w.id = $1)                          AS workspaces,
                (SELECT count(*) FROM actors a
                 WHERE a.actor_type = 'source'
                   AND ($1::uuid IS NULL OR a.workspace_id = $1))              AS sources,
                (SELECT count(*) FROM actors a
                 WHERE a.actor_type = 'destination'
                   AND ($1::uuid IS NULL OR a.workspace_id = $1))              AS destinations,
                (SELECT count(*) FROM connections c
                 WHERE $1::uuid IS NULL OR c.workspace_id = $1)                AS connections,
                (SELECT count(*) FROM jobs j JOIN connections c ON c.id = j.connection_id
                 WHERE $1::uuid IS NULL OR c.workspace_id = $1)                AS jobs_total,
                (SELECT count(*) FROM jobs j JOIN connections c ON c.id = j.connection_id
                 WHERE j.status = 'pending'
                   AND ($1::uuid IS NULL OR c.workspace_id = $1))              AS jobs_pending,
                (SELECT count(*) FROM jobs j JOIN connections c ON c.id = j.connection_id
                 WHERE j.status = 'running'
                   AND ($1::uuid IS NULL OR c.workspace_id = $1))              AS jobs_running,
                (SELECT count(*) FROM jobs j JOIN connections c ON c.id = j.connection_id
                 WHERE j.status = 'succeeded'
                   AND j.completed_at > now() - interval '24 hours'
                   AND ($1::uuid IS NULL OR c.workspace_id = $1))              AS jobs_succeeded_24h,
                (SELECT count(*) FROM jobs j JOIN connections c ON c.id = j.connection_id
                 WHERE j.status = 'failed'
                   AND j.completed_at > now() - interval '24 hours'
                   AND ($1::uuid IS NULL OR c.workspace_id = $1))              AS jobs_failed_24h,
                (SELECT COALESCE(sum(a.records_synced), 0)::bigint
                 FROM attempts a
                 JOIN jobs j ON j.id = a.job_id
                 JOIN connections c ON c.id = j.connection_id
                 WHERE a.ended_at > now() - interval '24 hours'
                   AND ($1::uuid IS NULL OR c.workspace_id = $1))              AS records_synced_24h,
                (SELECT max(j.completed_at)
                 FROM jobs j JOIN connections c ON c.id = j.connection_id
                 WHERE j.status = 'succeeded'
                   AND ($1::uuid IS NULL OR c.workspace_id = $1))              AS last_success_at",
        )
        .bind(workspace_id)
        .fetch_one(self.pool)
        .await?)
    }

    /// Terminal-state transitions.
    pub async fn finish(&self, id: i64, status: &str) -> Result<Job, StoreError> {
        sqlx::query_as::<_, Job>(&format!(
            "UPDATE jobs SET status = $2, completed_at = now(), updated_at = now()
             WHERE id = $1 RETURNING {COLUMNS}"
        ))
        .bind(id)
        .bind(status)
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::NotFound("job"))
    }

    /// Put a failed job back in the queue with a retry delay.
    pub async fn reschedule(&self, id: i64, delay_seconds: i64) -> Result<Job, StoreError> {
        sqlx::query_as::<_, Job>(&format!(
            "UPDATE jobs SET status = 'pending',
                             scheduled_at = now() + make_interval(secs => $2::double precision),
                             updated_at = now()
             WHERE id = $1 RETURNING {COLUMNS}"
        ))
        .bind(id)
        .bind(delay_seconds)
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::NotFound("job"))
    }

    /// Cancel: pending jobs terminate immediately; running jobs get
    /// `cancel_requested` and the worker stops them at the next message.
    pub async fn cancel(&self, id: i64) -> Result<Job, StoreError> {
        sqlx::query_as::<_, Job>(&format!(
            "UPDATE jobs SET
                 cancel_requested = true,
                 status = CASE WHEN status = 'pending' THEN 'cancelled' ELSE status END,
                 completed_at = CASE WHEN status = 'pending' THEN now() ELSE completed_at END,
                 updated_at = now()
             WHERE id = $1 AND status IN ('pending', 'running')
             RETURNING {COLUMNS}"
        ))
        .bind(id)
        .fetch_optional(self.pool)
        .await?
        .ok_or(StoreError::Conflict(
            "job is not pending or running".to_string(),
        ))
    }

    pub async fn cancel_requested(&self, id: i64) -> Result<bool, StoreError> {
        let row: Option<(bool,)> =
            sqlx::query_as("SELECT cancel_requested FROM jobs WHERE id = $1")
                .bind(id)
                .fetch_optional(self.pool)
                .await?;
        Ok(row.map(|(c,)| c).unwrap_or(false))
    }

    /// Requeue running jobs whose attempt heartbeat went stale (crashed
    /// worker). Returns the number of jobs reaped.
    pub async fn reap_stale(&self, stale_seconds: i64) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await?;
        let reaped: Vec<(i64,)> = sqlx::query_as(
            "UPDATE attempts SET status = 'failed', ended_at = now()
             WHERE status = 'running'
               AND last_heartbeat_at < now() - make_interval(secs => $1::double precision)
             RETURNING job_id",
        )
        .bind(stale_seconds)
        .fetch_all(&mut *tx)
        .await?;
        for (job_id,) in &reaped {
            sqlx::query(
                "UPDATE jobs SET status = 'pending', scheduled_at = now(), updated_at = now()
                 WHERE id = $1 AND status = 'running'",
            )
            .bind(job_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(reaped.len() as u64)
    }

    // ---- attempts ----

    pub async fn create_attempt(&self, job_id: i64) -> Result<Attempt, StoreError> {
        sqlx::query_as::<_, Attempt>(&format!(
            "INSERT INTO attempts (job_id, attempt_number)
             VALUES ($1, (SELECT COALESCE(MAX(attempt_number), 0) + 1
                          FROM attempts WHERE job_id = $1))
             RETURNING {ATTEMPT_COLUMNS}"
        ))
        .bind(job_id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StoreError::from_db(e, "attempt"))
    }

    pub async fn heartbeat(&self, attempt_id: i64) -> Result<(), StoreError> {
        sqlx::query("UPDATE attempts SET last_heartbeat_at = now() WHERE id = $1")
            .bind(attempt_id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    pub async fn finish_attempt(
        &self,
        attempt_id: i64,
        status: &str,
        records_synced: Option<i64>,
        state: Option<&Value>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE attempts SET status = $2, records_synced = $3, state = $4, ended_at = now()
             WHERE id = $1",
        )
        .bind(attempt_id)
        .bind(status)
        .bind(records_synced)
        .bind(state.map(|s| Json(s.clone())))
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_attempts(&self, job_id: i64) -> Result<Vec<Attempt>, StoreError> {
        Ok(sqlx::query_as::<_, Attempt>(&format!(
            "SELECT {ATTEMPT_COLUMNS} FROM attempts WHERE job_id = $1 ORDER BY attempt_number"
        ))
        .bind(job_id)
        .fetch_all(self.pool)
        .await?)
    }
}

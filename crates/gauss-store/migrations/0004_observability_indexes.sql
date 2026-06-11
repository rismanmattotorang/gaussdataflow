-- Indexes backing the fleet observability endpoints (`/api/v1/stats`,
-- `/api/v1/jobs`) and the queue's hot paths, so dashboards polling every few
-- seconds stay index-only as jobs/attempts history grows.

-- Cross-connection activity feed: ORDER BY created_at DESC LIMIT n.
CREATE INDEX jobs_created_idx ON jobs (created_at DESC);

-- claim_next: WHERE status = 'pending' AND scheduled_at <= now()
--             ORDER BY scheduled_at, id ... FOR UPDATE SKIP LOCKED.
CREATE INDEX jobs_pending_due_idx ON jobs (scheduled_at, id)
    WHERE status = 'pending';

-- 24h success/failure counters and last-success lookup.
CREATE INDEX jobs_status_completed_idx ON jobs (status, completed_at DESC);

-- Per-job attempt listing and latest-attempt record counts.
CREATE INDEX attempts_job_idx ON attempts (job_id, attempt_number DESC);

-- 24h records-moved aggregate.
CREATE INDEX attempts_ended_idx ON attempts (ended_at DESC);

-- Stale-worker reaping: running attempts with old heartbeats.
CREATE INDEX attempts_running_heartbeat_idx ON attempts (last_heartbeat_at)
    WHERE status = 'running';

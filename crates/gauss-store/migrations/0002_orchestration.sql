-- Phase 3: job orchestration columns, single-active-job guarantee,
-- heartbeats, and persisted per-connection sync state.

ALTER TABLE jobs
    ADD COLUMN scheduled_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN started_at       TIMESTAMPTZ,
    ADD COLUMN completed_at     TIMESTAMPTZ,
    ADD COLUMN cancel_requested BOOLEAN NOT NULL DEFAULT false;

-- At most one queued-or-running job per connection.
CREATE UNIQUE INDEX jobs_one_active_per_connection
    ON jobs (connection_id)
    WHERE status IN ('pending', 'running');

ALTER TABLE attempts
    ADD COLUMN last_heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- Latest committed sync state per connection (JSON array of protocol state
-- messages, one per stream). Written on every destination-acked checkpoint.
CREATE TABLE connection_states (
    connection_id UUID PRIMARY KEY REFERENCES connections (id) ON DELETE CASCADE,
    state         JSONB NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

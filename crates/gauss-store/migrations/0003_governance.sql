-- Phase 6: API tokens (RBAC), audit log, per-connection notifications.

CREATE TABLE api_tokens (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT NOT NULL UNIQUE,
    -- SHA-256 hex of the raw token; the raw value is shown once at creation
    -- and never stored.
    token_hash    TEXT NOT NULL UNIQUE,
    role          TEXT NOT NULL CHECK (role IN ('admin', 'editor', 'viewer')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at  TIMESTAMPTZ
);

CREATE TABLE audit_log (
    id          BIGSERIAL PRIMARY KEY,
    subject     TEXT NOT NULL,
    method      TEXT NOT NULL,
    path        TEXT NOT NULL,
    status      INT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_log_created_idx ON audit_log (created_at DESC);

-- Optional per-connection notification settings, e.g.
-- {"webhookUrl": "https://hooks.example.com/jobs"}.
ALTER TABLE connections ADD COLUMN notifications JSONB;

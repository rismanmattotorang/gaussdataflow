-- Phase 2 schema: workspaces, connector registry, actors, connections,
-- secrets, and the job tables Phase 3 will drive.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TYPE actor_type AS ENUM ('source', 'destination');

CREATE TABLE workspaces (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The connector registry: one row per known connector (definition), ingested
-- from a registry document or registered manually.
CREATE TABLE actor_definitions (
    id                 UUID PRIMARY KEY,
    actor_type         actor_type NOT NULL,
    name               TEXT NOT NULL,
    docker_repository  TEXT NOT NULL,
    docker_image_tag   TEXT NOT NULL,
    documentation_url  TEXT,
    spec               JSONB,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (actor_type, docker_repository)
);

-- Configured connector instances (Airbyte's "sources" and "destinations").
-- `configuration` is always the redacted form: secret values live in the
-- secrets table and are referenced by `{"_secret": "<id>"}` nodes.
CREATE TABLE actors (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id   UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    definition_id  UUID NOT NULL REFERENCES actor_definitions (id),
    actor_type     actor_type NOT NULL,
    name           TEXT NOT NULL,
    configuration  JSONB NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX actors_workspace_idx ON actors (workspace_id, actor_type);

CREATE TABLE connections (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    source_id       UUID NOT NULL REFERENCES actors (id) ON DELETE CASCADE,
    destination_id  UUID NOT NULL REFERENCES actors (id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'inactive', 'deprecated')),
    -- ConfiguredAirbyteCatalog (gauss-protocol wire form)
    catalog         JSONB NOT NULL,
    schedule        JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX connections_workspace_idx ON connections (workspace_id);

-- Local secrets backend (dev default). Values are opaque to the schema;
-- production deployments swap in an external backend via gauss-secrets.
CREATE TABLE secrets (
    id          TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Jobs/attempts: created in Phase 2 for schema stability, driven by the
-- Phase 3 orchestrator.
CREATE TABLE jobs (
    id             BIGSERIAL PRIMARY KEY,
    connection_id  UUID NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    job_type       TEXT NOT NULL CHECK (job_type IN ('sync', 'reset')),
    status         TEXT NOT NULL DEFAULT 'pending'
                   CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX jobs_connection_idx ON jobs (connection_id, created_at DESC);

CREATE TABLE attempts (
    id              BIGSERIAL PRIMARY KEY,
    job_id          BIGINT NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    attempt_number  INT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'running'
                    CHECK (status IN ('running', 'succeeded', 'failed')),
    records_synced  BIGINT,
    state           JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at        TIMESTAMPTZ,
    UNIQUE (job_id, attempt_number)
);

# gaussdataflow — Porting Strategy: Airbyte → Rust + Next.js

**Status:** Living document · **Last updated:** 2026-06-10

gaussdataflow is a ground-up reimplementation of the [Airbyte](https://github.com/airbytehq/airbyte)
data-integration platform on a **Rust** backend and a **Next.js** frontend.

---

## 1. Why this port is feasible

Airbyte's single most important architectural decision works in our favor: the
**Airbyte Protocol**. Sources and destinations are independent programs
(usually Docker images) that speak newline-delimited JSON messages over
STDIN/STDOUT. The platform never links against connector code.

> **Consequence:** a Rust platform that speaks the Airbyte Protocol can run the
> entire existing ecosystem of 550+ connectors *unchanged*. We port the
> platform (~the hard 20%), not the connectors (~the long-tail 80%).

## 2. Licensing posture (clean-room rule)

| Airbyte component | License | Our approach |
|---|---|---|
| Airbyte Protocol (spec + JSON schemas) | MIT | Implement against the spec directly |
| Connectors + CDK | MIT / Elv2 mix | Run published images as black boxes via the protocol |
| Platform (server, workers, webapp) | **Elastic License v2** | **No code copying.** Clean-room reimplementation from public docs, API specs, and observable behavior |

gaussdataflow itself is MIT. Contributors must not copy ELv2-licensed platform
source into this repository. Behavior parity is derived from the protocol spec,
the published OpenAPI config-API spec, and documentation.

## 3. Component mapping

| Airbyte (Java/Kotlin/Python/React) | gaussdataflow (Rust/TypeScript) | Phase |
|---|---|---|
| `airbyte-protocol` (JSON Schema) | `crates/gauss-protocol` (serde types) | 1 |
| Connector launcher in `airbyte-workers` | `crates/gauss-connector-runtime` | 1 |
| Connector dev loop (`spec/check/discover/read`) | `crates/gauss-cli` | 1 |
| `airbyte-db` (Postgres + jOOQ) | `gauss-store` (Postgres + sqlx) | 2 |
| `airbyte-server` config API (Micronaut) | `gauss-server` (axum) | 2 |
| Secrets persistence | `gauss-secrets` (file/env now; vault later) | 2 |
| Temporal workflows in `airbyte-workers` | `gauss-orchestrator` (Rust-native job queue on Postgres; Temporal optional later) | 3 |
| Sync "replication worker" (source→dest piping) | `gauss-sync` (tokio pipelines, state checkpointing) | 3 |
| `airbyte-webapp` (React + Vite) | `web/` (Next.js App Router) | 4 |
| Python/low-code CDK | `gauss-cdk` (Rust connector SDK) + declarative-manifest interpreter | 5 |
| OAuth, RBAC, notifications, migrations | hardening track | 6 |

## 4. Phase plan

### Phase 0 — Foundations (this milestone)
- [x] Monorepo scaffold: Cargo workspace + `web/` Next.js app
- [x] CI: fmt, clippy, tests, web build (GitHub Actions)
- [x] Strategy & licensing posture documented (this file)
- [x] Pin the protocol target: **Airbyte Protocol v0 series** (current published
      JSON schemas, including STATE v2 per-stream/global, TRACE, CONTROL)

**Exit criteria:** `cargo test` and `npm run build` green in CI on a clean clone.

### Phase 1 — Protocol & connector runtime (this milestone)
- [x] `gauss-protocol`: full message model (`RECORD`, `STATE`, `LOG`, `SPEC`,
      `CONNECTION_STATUS`, `CATALOG`, `TRACE`, `CONTROL`), catalogs, configured
      catalogs, sync modes — round-trip serialization tested against
      protocol-doc fixtures
- [x] `gauss-connector-runtime`: launch a connector as a Docker container or a
      local process, stream/parse its STDOUT into typed messages, forward
      STDERR to logs, surface `TRACE/ERROR` failures
- [x] `gauss-cli`: `gauss spec | check | discover | read` against any
      Airbyte-compatible connector image (`--image airbyte/source-faker:6.x`)
      or local binary (`--exec`)
- [x] `gauss-mock-connector`: a protocol-complete source written in Rust —
      doubles as the e2e test fixture and the seed of the Phase-5 CDK

**Exit criteria:** `gauss read --exec <mock> …` and (where Docker is
available) `gauss read --image airbyte/source-faker …` produce records, state
checkpoints, and stream-status traces.

### Phase 2 — Persistence & Config API (done)
- [x] Postgres schema (sqlx migrations): workspaces, actor definitions
      (connector registry), actors (sources/destinations), connections,
      secrets, jobs/attempts (driven in Phase 3)
- [x] `gauss-store`: typed repositories over a PgPool; runtime-checked
      queries so the crate builds without a live database
- [x] `gauss-secrets`: envelope abstraction — configs are split against the
      spec's `airbyte_secret` markers into a redacted form (persisted,
      API-visible) plus raw values behind a pluggable `SecretsBackend`
      (Postgres-local backend now; vault later). Hydration happens only at
      connector launch.
- [x] `gauss-server` (axum): config API compatible *in shape* with Airbyte's
      public API — workspaces, source/destination definitions, sources,
      destinations, connections, plus `POST /sources/{id}/check` which
      hydrates the config and runs the connector via the Phase-1 runtime
- [x] Connector registry ingestion (`POST /api/v1/definitions/import` +
      `--seed-registry` at boot) accepting Airbyte-registry-shaped JSON;
      idempotent upserts keyed on docker repository

**Exit criteria (met):** API integration tests green against real Postgres;
secrets never appear in API responses or the database's actor rows; `check`
runs a real containerized connector end-to-end through the API.

### Phase 3 — Orchestration & sync (done)
- [x] Job queue on Postgres (`FOR UPDATE SKIP LOCKED` in `gauss-store`) — no
      JVM Temporal dependency; any number of workers, no coordinator. A
      partial unique index guarantees one pending/running job per connection.
- [x] `gauss-sync` replication worker: source stdout → destination stdin with
      OS-pipe backpressure; destination stdout drained independently so the
      ack path can never deadlock the record path; checkpoints fire only on
      **destination-acked** STATE messages; per-stream status tracking; idle
      timeouts; watch-channel cancellation
- [x] `gauss-orchestrator`: claims jobs, hydrates configs, runs syncs,
      persists checkpoints to `connection_states` mid-flight, retries with
      exponential backoff up to `max_attempts`, heartbeats every attempt and
      reaps stale jobs from crashed workers back into the queue
- [x] Scheduling: `{"intervalMinutes": N}` and `{"cron": "<expr>"}` (5-field
      or seconds-resolution) schedules on connections; manual trigger
      (`POST /connections/{id}/sync`), job listing/inspection, and
      cancellation (pending jobs die immediately; running jobs stop at the
      next message) via the API; `gauss-server --worker` runs the
      orchestrator in-process
- [x] `exec:<path>` launcher scheme: run a connector as a local binary
      instead of a container — hermetic tests now, native Rust connectors in
      Phase 5

**Exit criteria (met):** an API-triggered job is claimed by the worker,
replicates source→destination with mid-flight checkpoints, persists resumable
per-stream state (verified: second sync reads zero already-synced records),
retries transient failures, and honors cancellation.

### Phase 4 — Web app (Next.js)
- App Router + React Server Components against `gauss-server`
- Core flows in order: connector setup (spec-driven JSON-schema forms — the
  hardest UI piece), connection create/edit (stream selection, sync modes),
  job history + live logs (SSE), workspace settings
- Typed API client generated from the server's OpenAPI document

### Phase 5 — Rust CDK & declarative connectors
- Extract `gauss-cdk` from the mock connector: traits for `Source`
  (`spec/check/discover/read`) and `Destination` (`spec/check/write`), state
  helpers, schema helpers, test harness
- Interpreter for Airbyte's declarative (low-code YAML) manifests — this
  unlocks the hundreds of manifest-only connectors *natively in Rust*, no
  Python runtime
- Native (non-Docker) execution path for Rust connectors: in-process or
  subprocess, 10–100× lighter than container-per-sync

### Phase 6 — Parity & hardening
- OAuth flows for connectors, RBAC/multi-workspace, notifications/webhooks
- Migration tooling: import config from an existing Airbyte deployment
  (config DB export → gauss-store)
- Performance benchmarks vs. Airbyte OSS (records/s, memory, cold-start)

## 5. Key technical decisions

| Decision | Choice | Rationale |
|---|---|---|
| Async runtime | tokio | ecosystem default; process + IO + channels |
| HTTP server | axum | tower ecosystem, OpenAPI via utoipa |
| DB | Postgres + sqlx | compile-time-checked queries; same DB Airbyte uses |
| Orchestration | Postgres-backed queue | one less moving part vs. Temporal; Airbyte itself moved toward simplification here |
| Protocol compat | wire-exact JSON | reuse connector ecosystem + Airbyte docs |
| Frontend | Next.js App Router, TypeScript strict | requested stack; RSC for log streaming pages |
| Connector execution | Docker first, native Rust later | ecosystem reuse now, performance later |

## 6. Risks & mitigations

- **Protocol drift** (Airbyte evolves the spec): serde models ignore unknown
  fields; fixtures pinned per protocol release; add schema-conformance tests in CI.
- **JSON-schema-driven forms** are the webapp's hidden complexity: spike early
  in Phase 4 with real connector specs (Postgres, Stripe) before committing to
  a form library.
- **Destination semantics** (dedup, generations/refreshes) are subtle: defer
  `overwrite_dedup`/refresh support until Phase 3 sync core is stable; test
  against real destination images.
- **ELv2 contamination**: PR checklist + this doc; reviewers reject anything
  derived from platform source.

## 7. Repository layout

```
gaussdataflow/
├── crates/
│   ├── gauss-protocol/           # Airbyte Protocol message model (Phase 1)
│   ├── gauss-connector-runtime/  # Docker/process launcher + message streaming (Phase 1)
│   ├── gauss-cli/                # `gauss` dev CLI (Phase 1)
│   └── gauss-mock-connector/     # Rust source connector; e2e fixture (Phase 1)
├── web/                          # Next.js app (scaffolded in Phase 0; built out in Phase 4)
├── docs/                         # strategy, ADRs
└── .github/workflows/ci.yml
```

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

### Phase 4 — Web app & MCP gateway (done)
- [x] Next.js console (App Router, hand-rolled design system, no UI-kit
      dependency): workspace dashboard, spec-driven connector setup forms
      rendered live from each definition's `connectionSpecification` JSON
      Schema (secrets as password fields, defaults, required markers, nested
      objects), "test & save" running the connector's `check`, connection
      builder with source `discover` + stream/sync-mode selection +
      interval/cron schedules, connection detail with one-click sync,
      polling job monitor, cancellation, and committed-state inspection
- [x] Hand-written typed API client (`web/lib/api.ts`); OpenAPI generation
      deferred to Phase 6
- [x] Server support: `POST /sources/{id}/discover`, permissive CORS (until
      Phase-6 authn), `check`/`discover` honor the `exec:` launcher scheme
- [x] **MCP gateway** (`gauss-mcp`): the control plane as Model Context
      Protocol tools over stdio — 17 tools covering registry, actors
      (including live `check_source`/`discover_source`), connections, syncs,
      jobs, and state; same secret envelope as the API
- [x] Connector registry seed expanded to 35+ popular sources/destinations
      (Postgres, MySQL, MongoDB, Stripe, Salesforce, Shopify, BigQuery,
      Snowflake, ClickHouse, Pinecone, …)

**Exit criteria (met):** a user can go from empty database to a running,
monitored sync entirely in the browser; an MCP agent can do the same flow
end-to-end through the gateway (covered by integration tests).

### Phase 5 — Rust CDK & declarative connectors (done)
- [x] `gauss-cdk` extracted from the reference connector: async `Source`
      (`spec/check/discover/read`) and `Destination` (`spec/check/write`)
      traits, an `Emitter` with protocol helpers (records, per-stream state,
      stream-status/error traces) and a capture mode for connector tests,
      state-input helpers, and a CLI runner that turns any impl into a
      complete connector binary with correct wire behavior (check failures →
      FAILED status, read failures → ERROR trace + exit 1)
- [x] Reference connector (`gauss-mock-connector`) rewritten on the CDK —
      all pre-existing e2e/sync tests pass unchanged, proving the extraction
      is wire-faithful
- [x] `gauss-declarative`: low-code manifest engine — one registered binary
      (`exec:gauss-declarative`) runs any HTTP-API source described by a
      YAML/JSON manifest carried in the connector config (`manifest` key,
      flowing through the registry/secret-envelope/launcher machinery
      untouched). v0 scope: api-key/bearer/basic auth, `{{ config.* }}`
      interpolation (unknown refs are hard errors), dot-path record
      selectors, offset/page/cursor-token pagination, per-stream
      `cursor_field` incremental sync with high-water-mark checkpoints
- [x] Native (non-Docker) execution path: the `exec:` launcher scheme
      (landed in Phase 3) is now the production path for CDK and declarative
      connectors — no container per sync

**Exit criteria (met):** a manifest-only connector registered via `exec:`
syncs a live HTTP API through the full platform (check → discover →
scheduled job → records delivered → cursor checkpointed → second sync reads
zero), with no container anywhere in the pipeline.

### Phase 6 — Parity & hardening (done)
- [x] AuthN/Z: API tokens (SHA-256 hashed at rest, raw value shown once) with
      RBAC roles — viewer (read), editor (mutations), admin (token/audit
      management); `--require-auth` enforces on every `/api/v1` request,
      `--create-token name:role` bootstraps offline
- [x] Audit log: every mutating request recorded (subject, method, path,
      status) off the request path; `GET /api/v1/audit` (admin)
- [x] OAuth2 plumbing: provider-agnostic authorize-URL builder with
      single-use TTL'd CSRF states and server-side code-for-token exchange;
      returned access/refresh tokens are sealed into the secrets backend and
      surfaced only as `{"_secret": id}` references
- [x] Vault-backed secrets: `VaultSecretsBackend` (KV v2) behind the same
      `SecretsBackend` trait; `--secrets-backend vault` +
      `VAULT_ADDR`/`VAULT_TOKEN`
- [x] Notifications: per-connection `notifications.webhookUrl` posted on
      terminal jobs (status, records synced, attempt)
- [x] Migration tooling: `--import-file` applies a portable deployment
      document (workspace, definitions, actors with secrets sealed on
      import, scheduled connections)
- [x] Benchmark harness: `cargo test --test bench --release -- --ignored`
      measures end-to-end replication throughput (~56k records/s in a
      constrained container)

**Exit criteria (met):** with auth required, anonymous requests are 401,
viewer mutations are 403, admin actions are audited with their subject;
secrets round-trip through Vault; OAuth tokens never appear raw in any
response; a deployment document restores a working scheduled pipeline.

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

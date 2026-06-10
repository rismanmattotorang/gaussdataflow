# Gauss-DataFlow — Architecture & Design Decisions

**Maintained by Gaussian Technologies · MIT licensed, open source forever.**

Gauss-DataFlow is a data movement (ELT) platform with a Rust control and data
plane and a Next.js console. This document records the architecture, the key
design decisions, and the build history.

## 1. The connector model

Everything rests on one decision: **connectors are independent programs**, not
plugins. A source is any executable that answers four commands —
`spec`, `check --config`, `discover --config`, and
`read --config --catalog [--state]` — by emitting newline-delimited JSON
messages (the *Gauss connector protocol*) on STDOUT. A destination answers
`spec`, `check`, and `write`, consuming messages on STDIN and acknowledging
state checkpoints on STDOUT.

Consequences:

- The platform never links connector code; connectors can be written in any
  language and shipped as Docker images or native binaries.
- Third-party connectors that speak a compatible newline-delimited-JSON
  protocol run unchanged. Specs may mark secret fields with `gauss_secret`
  (the legacy `airbyte_secret` keyword is accepted for compatibility with
  existing third-party connector specs).
- The protocol message model lives in `gauss-protocol`: `RECORD`, `STATE`
  (per-stream, global, and legacy forms), `LOG`, `SPEC`, `CONNECTION_STATUS`,
  `CATALOG`, `TRACE`, and `CONTROL` envelopes, all wire-exact and tolerant of
  unknown fields in both directions.

## 2. System layout

```
                   ┌────────────────────────────────────────────────┐
  humans ──────▶   │  web/            Next.js console               │
                   └──────────────┬─────────────────────────────────┘
                   ┌──────────────▼─────────────────────────────────┐
  agents ──MCP──▶  │  gauss-mcp     │  gauss-server   (axum REST)   │
                   └──────────────┬─────────────────────────────────┘
                   ┌──────────────▼──────────────┐  ┌───────────────┐
                   │  gauss-store   (Postgres)   │◀─┤ gauss-secrets │
                   │  registry · actors ·        │  │ sealed creds  │
                   │  connections · job queue    │  └───────────────┘
                   └──────────────┬──────────────┘
                   ┌──────────────▼──────────────┐
                   │  gauss-orchestrator         │
                   └──────────────┬──────────────┘
                   ┌──────────────▼──────────────┐
                   │  gauss-sync (replication)   │
                   └──────────────┬──────────────┘
                   ┌──────────────▼──────────────┐
                   │  gauss-connector-runtime    │
                   │  + gauss-protocol           │
                   └─────────────────────────────┘
```

| Crate | Role |
|---|---|
| `gauss-protocol` | Wire-exact message model |
| `gauss-connector-runtime` | Docker / local-process / `exec:` launchers, typed message streaming |
| `gauss-sync` | Replication engine: backpressured piping, destination-acked checkpoints |
| `gauss-store` | Postgres persistence + the job queue (sqlx, embedded migrations) |
| `gauss-secrets` | Secret envelope; Postgres or HashiCorp Vault backends |
| `gauss-orchestrator` | Job execution, retries, heartbeats, schedules, cancellation, webhooks |
| `gauss-server` | REST control plane, auth/RBAC/audit, OAuth2 plumbing, import tooling |
| `gauss-mcp` | Model Context Protocol gateway for AI agents (stdio) |
| `gauss-cdk` | Connector Development Kit (`Source`/`Destination` traits + binary runner) |
| `gauss-declarative` | Low-code engine: YAML manifests → native HTTP-API sources |
| `gauss-cli` | Connector dev loop (`spec/check/discover/read`) |
| `gauss-mock-connector` | Reference connector; hermetic e2e fixture |

## 3. Key design decisions

| Decision | Choice | Rationale |
|---|---|---|
| Async runtime | tokio | process + IO + channels in one ecosystem |
| HTTP server | axum | tower middleware (auth, CORS, tracing) |
| Database | Postgres + sqlx | runtime-checked queries; one operational dependency |
| Job queue | Postgres `FOR UPDATE SKIP LOCKED` | N workers, zero coordinators/brokers/workflow engines; a partial unique index guarantees one active job per connection |
| Checkpointing | destination-acked STATE only | state can never run ahead of delivered data; crash-resume re-reads nothing it doesn't have to |
| Backpressure | the OS pipe itself | destination stdin write suspends the source pump; the ack path is drained independently so it can never deadlock the record path |
| Secrets | envelope + pluggable backend | configs are split on entry; only `{"_secret": id}` references are persisted, returned, or logged; hydration happens in memory at connector launch |
| Connector execution | Docker, plus `exec:` native path | ecosystem compatibility and container-free CDK/declarative connectors |
| Frontend | Next.js App Router, strict TS, no UI kit | spec-driven forms rendered from connector JSON Schemas |
| Agent access | MCP over stdio | the entire control plane as typed, validated tools |

## 4. Reliability machinery

- **Retries**: failed attempts reschedule with exponential backoff up to
  `max_attempts`; attempt history is preserved per job.
- **Heartbeats & reaping**: running attempts heartbeat continuously; jobs
  whose worker died are reaped back into the queue automatically.
- **Cancellation**: pending jobs die immediately; running jobs observe a
  cancel flag and stop at the next message boundary (children are killed on
  drop).
- **Schedules**: `{"intervalMinutes": N}` or `{"cron": "<expr>"}` per
  connection; enqueueing is idempotent under the unique index.
- **Notifications**: terminal jobs POST to a per-connection webhook.

## 5. Security model

- **API tokens** (SHA-256 hashed at rest, raw value shown once) with three
  roles: viewer (read), editor (mutations), admin (token/audit management).
  `--require-auth` enforces on every `/api/v1` request.
- **Audit log**: every mutation recorded (subject, method, path, status).
- **OAuth2 plumbing**: server-side CSRF state issuance and code-for-token
  exchange; returned tokens are sealed before they reach the caller.
- **Vault**: secrets can live in HashiCorp Vault (KV v2) instead of Postgres
  via `--secrets-backend vault`.

## 6. Build history

| Phase | Delivered |
|---|---|
| 0 | Monorepo, CI (fmt/clippy/tests + Postgres service, web build) |
| 1 | Protocol model, connector runtime, dev CLI, reference connector |
| 2 | Postgres persistence, secret envelope, REST control plane |
| 3 | Postgres-native job orchestration, replication engine, schedules |
| 4 | Web console, MCP gateway, registry tooling |
| 5 | Rust CDK, declarative low-code engine, container-free execution |
| 6 | RBAC tokens, audit, OAuth2, Vault, webhooks, import, benchmarks |

## 7. Licensing

MIT, applying to every crate and the web console. Gauss-DataFlow is committed
to remaining fully open source — no license switches, no open-core
carve-outs.

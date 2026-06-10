<div align="center">

# ⚡ Gauss-DataFlow

**The data movement platform for the agentic era.**

Built in the open by **Gaussian Technologies**.

Move data from anywhere to anywhere — orchestrated by a Rust core that treats
reliability as physics, operated by humans through a modern web console, and by
AI agents through a native MCP gateway.

[Quickstart](#quickstart) · [Architecture](#architecture) · [MCP Gateway](#-built-for-agents-mcp-gateway) · [Architecture](docs/ARCHITECTURE.md) · MIT License

</div>

---

## Why Gauss-DataFlow

Data integration platforms were built for a world where humans click buttons
and JVMs burn memory. Gauss-DataFlow is built for what comes next:

- **🦀 A Rust data plane.** The entire control and data plane — API, scheduler,
  replication engine, secrets — is a single, memory-safe, natively compiled
  stack. No JVM, no workflow-engine sidecar, no garbage-collection pauses in
  the middle of your sync. One binary per role, milliseconds to start.
- **🤖 Agent-native by design.** Every operation a human can do in the console,
  an AI agent can do through the built-in
  [Model Context Protocol](https://modelcontextprotocol.io) gateway: browse
  connectors, configure sources, wire pipelines, trigger and monitor syncs.
  Your data platform becomes a tool your agents call.
- **🔌 An open connector model.** Connectors are independent programs speaking
  newline-delimited JSON over STDIN/STDOUT — the Gauss connector protocol.
  Any protocol-compatible connector runs unchanged, as a Docker image or a
  native binary; the registry imports third-party catalogs with one call.
- **🛡️ Secrets that never leak.** Connector configs are split against their
  spec the moment they enter the system: secret fields are sealed into a
  dedicated backend and replaced by opaque references. The API, the database
  rows, the logs — none of them ever see a raw credential. Hydration happens
  only in memory, only at connector launch.
- **🧠 Exactly-resumable syncs.** Checkpoints are committed only when the
  destination acknowledges them — state can never run ahead of data. Kill a
  worker mid-sync, lose a node, cancel a job: the next run resumes from the
  last destination-confirmed cursor and re-reads nothing it doesn't have to.
- **🗄️ Postgres is the only dependency.** The job queue *is* a Postgres table
  (`FOR UPDATE SKIP LOCKED`). Scale workers horizontally with zero
  coordinators, brokers, or workflow engines. Heartbeats reap work from
  crashed nodes automatically; retries back off exponentially; schedules are
  cron or interval.

## What you get

| | |
|---|---|
| **Web console** | Next.js app: mission-control dashboard (fleet pulse, live activity feed), workspaces, spec-driven connector setup forms (rendered live from each connector's JSON Schema), stream-level connection builder with discovery, one-click sync, live job monitoring with attempt drill-down, committed-state inspection, toasts and guarded deletes |
| **Terminal console** | `gauss-tui` — a [Ratatui](https://ratatui.rs) console for the whole control plane: fleet stats, workspace tabs, job tables with live refresh, one-key sync/cancel, state inspection — over the same REST API, local or remote |
| **REST API** | Full control plane at `/api/v1/*`: workspaces, connector registry, sources/destinations (with `check` + `discover`), connections, jobs (incl. cross-connection activity feed), state, and aggregate `stats` |
| **MCP gateway** | `gauss-mcp` — 19 annotated tools over stdio with structured output and protocol negotiation through `2025-06-18`; plug into Claude Desktop, Claude Code, or any MCP client |
| **Security & governance** | API tokens with RBAC (admin/editor/viewer), audit log of every mutation, generic OAuth2 plumbing with sealed tokens, secrets in Postgres or HashiCorp Vault |
| **Orchestrator** | Postgres-backed queue, scheduler (cron + interval), retries, heartbeats, cancellation — embedded in the server (`--worker`) or scaled out as separate processes |
| **Replication engine** | `gauss-sync` — pipe-backpressured source→destination streaming with destination-acked checkpointing |
| **Connector registry** | Import any registry document with one API call; register connectors by Docker image or native binary |
| **Rust CDK** | `gauss-cdk` — implement two traits, get a complete protocol-correct connector binary; container-free execution via the `exec:` launcher |
| **Low-code engine** | `gauss-declarative` — describe an HTTP API in a YAML manifest (auth, pagination, incremental cursors) and run it as a native source: no container, no code |
| **Dev CLI** | `gauss spec\|check\|discover\|read` — the connector development loop against any image or binary |
| **Operations** | Job webhooks on completion, one-command deployment import, replication benchmark harness |

## Quickstart

Prereqs: Rust (stable), Node 22+, Postgres 14+, Docker (for containerized connectors).

```sh
git clone https://github.com/rismanmattotorang/gaussdataflow && cd gaussdataflow
cargo build --workspace --release

# 1. Boot the platform (API + orchestration worker; migrations run automatically)
export DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/gauss
./target/release/gauss-server --worker \
    --seed-registry crates/gauss-server/seed/registry.json

# 2. Launch the console
cd web && npm install && npm run dev   # → http://localhost:3000

# …or stay in the terminal
./target/release/gauss-tui             # → full TUI against http://127.0.0.1:8000
```

Create a workspace, add a source and a destination, discover streams, hit
**Sync now**, and watch the job stream records.

Prefer a terminal dashboard? `gauss-tui` gives you the fleet pulse
(pipelines, queue depth, 24-hour success/failure, records moved), workspace
tabs for connections/jobs/sources/destinations, one-key sync triggering and
job cancellation, attempt history, and committed-state inspection — against
any deployment: `gauss-tui --api https://gauss.internal:8000 --token gauss_…`.

Prefer the terminal? The whole flow is four `curl`s:

```sh
B=http://127.0.0.1:8000/api/v1
WS=$(curl -s -X POST $B/workspaces -H 'content-type: application/json' \
     -d '{"name":"prod"}' | jq -r .workspaceId)
# …create a source & destination, then:
curl -s -X POST $B/connections/$CONN/sync          # trigger
curl -s $B/jobs/1                                  # job + attempts
curl -s $B/connections/$CONN/state                 # committed cursors
```

## 🤖 Built for agents: MCP gateway

Give any MCP client operational control of your data platform:

```json
{
  "mcpServers": {
    "gauss-dataflow": {
      "command": "/path/to/gauss-mcp",
      "env": { "DATABASE_URL": "postgres://postgres:postgres@127.0.0.1:5432/gauss" }
    }
  }
}
```

Then just ask: *“Set up a sync from our Postgres to the warehouse, hourly,
incremental on `updated_at` — and tell me when the first run finishes.”* The
agent browses the registry, configures the source (secrets sealed
automatically), discovers streams, creates the connection with a cron
schedule, triggers the job, and polls it — through typed, validated tools:

`get_platform_stats` · `list_recent_jobs` · `list_workspaces` ·
`create_workspace` · `list_connector_definitions` · `register_connector` ·
`create_source` · `create_destination` · `list_sources` ·
`list_destinations` · `check_source` · `discover_source` ·
`create_connection` · `list_connections` · `trigger_sync` · `list_jobs` ·
`get_job` · `cancel_job` · `get_connection_state`

Every tool carries MCP behaviour annotations (`readOnlyHint`,
`destructiveHint`, `idempotentHint`, `openWorldHint`) so clients can apply
least-privilege policy, results include `structuredContent` alongside text,
and initialization negotiates protocol revisions `2024-11-05` through
`2025-06-18`. The forward plan — streamable HTTP at `/mcp`, OAuth 2.1,
role-scoped virtual tool surfaces, an AI connector builder targeting the
declarative engine — lives in [docs/ROADMAP.md](docs/ROADMAP.md).

## 🔐 Locked down by default-deny

Flip on `--require-auth` and every API request needs a bearer token; tokens
carry roles and only their SHA-256 hash ever touches the database:

```sh
gauss-server --create-token ops:admin        # prints the token once
gauss-server --create-token ci:editor       # mutations, no token management
gauss-server --create-token dashboards:viewer  # read-only
gauss-server --require-auth --worker
```

Every mutation lands in the audit log (`GET /api/v1/audit`) with its subject,
path, and outcome. Raw secret values can live in Postgres (default) or
**HashiCorp Vault** (`--secrets-backend vault` + `VAULT_ADDR`/`VAULT_TOKEN`) —
either way the API, rows, and logs only ever see `{"_secret": id}`
references. For connectors that authenticate users via OAuth2, the server
does the parts a browser must not: CSRF state issuance and the
code-for-token exchange, sealing the returned tokens before they reach the
caller (`POST /api/v1/oauth/authorize_url`, `POST /api/v1/oauth/complete`).

Moving in? `gauss-server --import-file deployment.json` bootstraps a
workspace — definitions, configured sources/destinations (secrets sealed on
import), and scheduled connections — from one portable document. Job
completion can ping your systems back: set
`{"webhookUrl": "https://…"}` in a connection's `notifications`.

## 🧩 Connectors without containers

Describe an HTTP API in a manifest; Gauss-DataFlow runs it as a **native
source** — no container, no glue code:

```yaml
requester:
  url_base: https://api.example.com
  authenticator: { type: bearer, api_token: "{{ config.api_key }}" }
streams:
  - name: orders
    path: /v1/orders
    record_selector: data
    primary_key: [id]
    cursor_field: updated_at          # incremental sync, checkpointed
    paginator: { type: offset, page_size: 100 }
```

Register the `gauss-declarative` binary once (`exec:/path/to/gauss-declarative`);
every source configured from it carries its own manifest plus credentials —
secrets sealed like any other connector. Auth (api-key/bearer/basic),
offset/page/cursor pagination, and high-water-mark incremental cursors are
built in.

Need full control? Implement two traits with **`gauss-cdk`** and you have a
protocol-correct connector binary — `spec/check/discover/read/write` argument
handling, wire output, error-to-trace conversion, and exit codes all come
from the runner:

```rust
#[async_trait::async_trait]
impl gauss_cdk::Source for MyApi { /* spec, check, discover, read */ }

#[tokio::main]
async fn main() -> std::process::ExitCode {
    gauss_cdk::cli::run_source(MyApi).await
}
```

## Architecture

```
                   ┌────────────────────────────────────────────────┐
  humans ──────▶   │  web/ console (Next.js) · gauss-tui (Ratatui)  │
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
                   │  gauss-orchestrator         │  claim → retry →
                   │  (N workers, no coordinator)│  heartbeat → reap
                   └──────────────┬──────────────┘
                   ┌──────────────▼──────────────┐
                   │  gauss-sync                 │  src ─▶ dst piping,
                   │  replication engine         │  acked checkpoints
                   └──────────────┬──────────────┘
                   ┌──────────────▼──────────────┐
                   │  gauss-connector-runtime    │  docker | exec:
                   │  + gauss-protocol           │  open wire protocol
                   └─────────────────────────────┘
```

| Crate | Role |
|---|---|
| `gauss-protocol` | Wire-exact model of the open connector protocol (records, state, catalogs, traces) |
| `gauss-connector-runtime` | Launches connectors (Docker image or `exec:` native binary), streams typed messages |
| `gauss-sync` | Replication engine: backpressured piping, destination-acked checkpointing |
| `gauss-store` | Postgres persistence + the job queue (sqlx, embedded migrations) |
| `gauss-secrets` | Secret envelope: seal on write, hydrate in-memory at launch only |
| `gauss-orchestrator` | Job execution, retries, heartbeats, schedules, cancellation |
| `gauss-server` | REST control plane (+ embedded worker with `--worker`) |
| `gauss-mcp` | MCP gateway for AI agents (stdio) |
| `gauss-cdk` | Connector Development Kit: `Source`/`Destination` traits + a runner that yields a complete connector binary |
| `gauss-declarative` | Low-code engine: YAML manifests → native HTTP-API sources (auth, pagination, incremental) |
| `gauss-cli` | Connector dev loop |
| `gauss-tui` | Ratatui terminal console: fleet pulse, pipelines, jobs, one-key operations |
| `gauss-mock-connector` | Reference connector built on the CDK; powers the hermetic e2e suite |

## Reliability, tested

`cargo test --workspace` runs 50+ tests, including end-to-end replication
through real OS processes: full syncs, incremental resume from committed
cursors, mid-flight cancellation, crash-retry with backoff, duplicate-job
rejection, schedule evaluation, secret redaction/rotation, and the complete
MCP agent flow. Integration tests provision throwaway Postgres databases per
test and skip gracefully when `DATABASE_URL` is unset; CI runs everything
against Postgres 16.

```sh
DATABASE_URL=postgres://… cargo test --workspace
```

There's a benchmark harness too — on a modest container the replication
engine moves **~56k records/s (≈10 MiB/s)** through two real connector
processes with full protocol serialization and destination-acked
checkpoints:

```sh
cargo test -p gauss-mock-connector --test bench --release -- --ignored --nocapture
```

## Status

All six phases of the founding roadmap have shipped — wire protocol &
connector runtime, persistence & sealed secrets, Postgres-native
orchestration, the web console & MCP gateway, the Rust CDK & declarative
engine, and enterprise hardening (RBAC, audit, OAuth2, Vault, webhooks,
import tooling) — plus Phase 7: the `gauss-tui` terminal console, the
mission-control dashboard, fleet observability APIs, and the
annotation-rich MCP gateway. Architecture and design decisions live in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md); the researched forward roadmap
for the MCP gateway and agentic AI integration lives in
[docs/ROADMAP.md](docs/ROADMAP.md).

## License

**MIT, forever.** Gauss-DataFlow is and will remain fully open source —
no license switches, no open-core carve-outs. Copyright © 2026
[Gaussian Technologies](https://github.com/rismanmattotorang/gaussdataflow);
issues and PRs welcome.

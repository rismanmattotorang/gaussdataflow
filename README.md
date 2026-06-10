<div align="center">

# ⚡ gaussdataflow

**The data movement platform for the agentic era.**

Move data from anywhere to anywhere — orchestrated by a Rust core that treats
reliability as physics, operated by humans through a modern web console, and by
AI agents through a native MCP gateway.

[Quickstart](#quickstart) · [Architecture](#architecture) · [MCP Gateway](#-built-for-agents-mcp-gateway) · [Roadmap](docs/STRATEGY.md) · MIT License

</div>

---

## Why gaussdataflow

Data integration platforms were built for a world where humans click buttons
and JVMs burn memory. gaussdataflow is built for what comes next:

- **🦀 A Rust data plane.** The entire control and data plane — API, scheduler,
  replication engine, secrets — is a single, memory-safe, natively compiled
  stack. No JVM, no workflow-engine sidecar, no garbage-collection pauses in
  the middle of your sync. One binary per role, milliseconds to start.
- **🤖 Agent-native by design.** Every operation a human can do in the console,
  an AI agent can do through the built-in
  [Model Context Protocol](https://modelcontextprotocol.io) gateway: browse
  connectors, configure sources, wire pipelines, trigger and monitor syncs.
  Your data platform becomes a tool your agents call.
- **🔌 An open connector ecosystem from day one.** gaussdataflow speaks the
  open Airbyte Protocol on the wire, so hundreds of existing
  protocol-compatible connector images — Postgres, Stripe, Salesforce,
  Shopify, BigQuery, Snowflake, Pinecone, and the rest — run unchanged. Bring
  your own connector as a Docker image or a native binary.
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
| **Web console** | Next.js app: workspaces, spec-driven connector setup forms (rendered live from each connector's JSON Schema), stream-level connection builder with discovery, one-click sync, live job monitoring, committed-state inspection |
| **REST API** | Full control plane at `/api/v1/*`: workspaces, connector registry, sources/destinations (with `check` + `discover`), connections, jobs, state |
| **MCP gateway** | `gauss-mcp` — 17 tools over stdio; plug into Claude Desktop, Claude Code, or any MCP client |
| **Orchestrator** | Postgres-backed queue, scheduler (cron + interval), retries, heartbeats, cancellation — embedded in the server (`--worker`) or scaled out as separate processes |
| **Replication engine** | `gauss-sync` — pipe-backpressured source→destination streaming with destination-acked checkpointing |
| **Connector registry** | Seeded with 35+ popular sources & destinations; import the full public catalog with one API call; register anything by image or local binary |
| **Dev CLI** | `gauss spec\|check\|discover\|read` — the connector development loop against any image or binary |

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
```

Create a workspace, add a source and a destination, discover streams, hit
**Sync now**, and watch the job stream records.

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
    "gaussdataflow": {
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

`list_workspaces` · `create_workspace` · `list_connector_definitions` ·
`register_connector` · `create_source` · `create_destination` ·
`list_sources` · `list_destinations` · `check_source` · `discover_source` ·
`create_connection` · `list_connections` · `trigger_sync` · `list_jobs` ·
`get_job` · `cancel_job` · `get_connection_state`

## Architecture

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
| `gauss-cli` | Connector dev loop |
| `gauss-mock-connector` | Reference connector (source + destination) powering the hermetic e2e suite |

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

## Roadmap

Native (container-free) Rust connector SDK, declarative low-code connectors,
OAuth flows, RBAC, vault-backed secrets, and benchmarks — see
[docs/STRATEGY.md](docs/STRATEGY.md).

## License

MIT. Built in the open — issues and PRs welcome.

# gaussdataflow

An open-source data-integration (ELT) platform — a clean-room port of
[Airbyte](https://github.com/airbytehq/airbyte)'s architecture to a **Rust**
backend and a **Next.js** frontend.

gaussdataflow speaks the [Airbyte Protocol](https://docs.airbyte.com/understanding-airbyte/airbyte-protocol)
on the wire, so it can run the existing ecosystem of Airbyte-compatible
connector images unchanged.

**Read the plan:** [docs/STRATEGY.md](docs/STRATEGY.md) · **Current state:** Phase 0/1 complete.

## Layout

| Path | What it is |
|---|---|
| `crates/gauss-protocol` | Airbyte Protocol message model (serde types, wire-exact JSON) |
| `crates/gauss-connector-runtime` | Launches connectors (Docker or local process) and streams typed messages |
| `crates/gauss-cli` | `gauss` — connector dev loop: `spec`, `check`, `discover`, `read` |
| `crates/gauss-mock-connector` | A protocol-complete source connector in Rust; e2e test fixture |
| `web/` | Next.js app (UI lands in Phase 4) |

## Quickstart

```sh
# Build and test everything
cargo test --workspace

# Run the mock source end-to-end (no Docker needed)
cargo build --workspace
echo '{"record_count": 5}' > /tmp/config.json
./target/debug/gauss discover --exec ./target/debug/gauss-mock-connector --config /tmp/config.json
./target/debug/gauss read     --exec ./target/debug/gauss-mock-connector \
    --config /tmp/config.json --full-refresh

# Run a real Airbyte connector (requires Docker)
echo '{"count": 10}' > /tmp/faker.json
./target/debug/gauss spec  --image airbyte/source-faker:latest
./target/debug/gauss check --image airbyte/source-faker:latest --config /tmp/faker.json
./target/debug/gauss read  --image airbyte/source-faker:latest --config /tmp/faker.json --full-refresh

# Web app
cd web && npm install && npm run dev
```

## License

MIT. This project does **not** contain code derived from Airbyte's
ELv2-licensed platform source; see the licensing posture in
[docs/STRATEGY.md](docs/STRATEGY.md#2-licensing-posture-clean-room-rule).

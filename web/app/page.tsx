const phases = [
  {
    id: 0,
    name: "Foundations",
    status: "done",
    desc: "Monorepo, CI, strategy, protocol target pinned",
  },
  {
    id: 1,
    name: "Protocol & connector runtime",
    status: "done",
    desc: "gauss-protocol, Docker/process runtime, gauss CLI, mock connector",
  },
  {
    id: 2,
    name: "Persistence & Config API",
    status: "done",
    desc: "Postgres + sqlx, axum config API, secret envelope, connector registry",
  },
  {
    id: 3,
    name: "Orchestration & sync",
    status: "next",
    desc: "Job queue, replication worker, checkpointing, schedules",
  },
  {
    id: 4,
    name: "Web app",
    status: "later",
    desc: "Connection builder, spec-driven forms, job monitoring (this app)",
  },
  {
    id: 5,
    name: "Rust CDK & declarative connectors",
    status: "later",
    desc: "Native connector SDK, low-code manifest interpreter",
  },
  {
    id: 6,
    name: "Parity & hardening",
    status: "later",
    desc: "OAuth, RBAC, migration tooling, benchmarks",
  },
] as const;

const badgeLabel = { done: "done", next: "up next", later: "planned" } as const;

export default function Home() {
  return (
    <main>
      <h1>
        gauss<span>dataflow</span>
      </h1>
      <p className="lede">
        Open-source data integration, rebuilt on Rust and Next.js.
        Wire-compatible with the Airbyte Protocol, so the existing connector
        ecosystem runs unchanged: <code>gauss read --image
        airbyte/source-faker:latest …</code>
      </p>
      <ul className="phases">
        {phases.map((phase) => (
          <li key={phase.id}>
            <span className={`badge ${phase.status}`}>
              {badgeLabel[phase.status]}
            </span>
            <span>
              <strong>
                Phase {phase.id}: {phase.name}
              </strong>
              <br />
              <span className="desc">{phase.desc}</span>
            </span>
          </li>
        ))}
      </ul>
      <footer>
        The UI proper lands in Phase 4. Strategy:{" "}
        <a href="https://github.com/rismanmattotorang/gaussdataflow/blob/main/docs/STRATEGY.md">
          docs/STRATEGY.md
        </a>
      </footer>
    </main>
  );
}

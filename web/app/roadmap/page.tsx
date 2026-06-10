const phases = [
  {
    id: 0,
    name: "Foundations",
    status: "done",
    desc: "Monorepo, CI, protocol target pinned",
  },
  {
    id: 1,
    name: "Protocol & connector runtime",
    status: "done",
    desc: "Wire protocol, Docker/process runtime, dev CLI, reference connector",
  },
  {
    id: 2,
    name: "Persistence & Config API",
    status: "done",
    desc: "Postgres + sqlx, config API, secret envelope, connector registry",
  },
  {
    id: 3,
    name: "Orchestration & sync",
    status: "done",
    desc: "Postgres job queue, replication worker, checkpointing, schedules",
  },
  {
    id: 4,
    name: "Web app & MCP gateway",
    status: "done",
    desc: "Spec-driven connector forms, connection builder, job monitoring, MCP tools for agents",
  },
  {
    id: 5,
    name: "Native connector SDK",
    status: "done",
    desc: "Rust CDK, low-code manifest engine, container-free execution",
  },
  {
    id: 6,
    name: "Enterprise hardening",
    status: "done",
    desc: "API tokens + RBAC, audit log, OAuth2, Vault secrets, webhooks, import",
  },
] as const;

const badgeLabel = { done: "done", next: "up next", later: "planned" } as const;

export default function Roadmap() {
  return (
    <main>
      <h1>
        Gauss-<span>DataFlow</span> roadmap
      </h1>
      <p className="lede">
        Open-source data movement by Gaussian Technologies — built on Rust and
        Next.js. MIT licensed, forever.
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
    </main>
  );
}

"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { api } from "@/lib/api";
import { ErrorNote, StatusBadge, timeAgo, usePoll } from "@/components/ui";

export default function WorkspacePage() {
  const { id } = useParams<{ id: string }>();

  const { data: workspace } = usePoll(() => api.workspaces.get(id), null, [id]);
  const sources = usePoll(() => api.actors("sources").list(id), null, [id]);
  const destinations = usePoll(
    () => api.actors("destinations").list(id),
    null,
    [id],
  );
  const connections = usePoll(() => api.connections.list(id), 10000, [id]);

  return (
    <main>
      <h1>{workspace?.name ?? "Workspace"}</h1>
      <p className="lede">Sources, destinations, and connections.</p>
      <ErrorNote
        error={sources.error ?? destinations.error ?? connections.error}
      />

      <div className="grid-2">
        <section>
          <div className="form-row" style={{ justifyContent: "space-between" }}>
            <h2 style={{ margin: 0 }}>Sources</h2>
            <Link className="btn ghost" href={`/workspaces/${id}/new/source`}>
              + New source
            </Link>
          </div>
          {sources.data?.length === 0 && <p className="meta">None yet.</p>}
          {sources.data?.map((s) => (
            <div className="card" key={s.id}>
              <div className="row">
                <h3>{s.name}</h3>
                <span className="meta">created {timeAgo(s.createdAt)}</span>
              </div>
            </div>
          ))}
        </section>

        <section>
          <div className="form-row" style={{ justifyContent: "space-between" }}>
            <h2 style={{ margin: 0 }}>Destinations</h2>
            <Link
              className="btn ghost"
              href={`/workspaces/${id}/new/destination`}
            >
              + New destination
            </Link>
          </div>
          {destinations.data?.length === 0 && <p className="meta">None yet.</p>}
          {destinations.data?.map((d) => (
            <div className="card" key={d.id}>
              <div className="row">
                <h3>{d.name}</h3>
                <span className="meta">created {timeAgo(d.createdAt)}</span>
              </div>
            </div>
          ))}
        </section>
      </div>

      <div className="form-row" style={{ justifyContent: "space-between" }}>
        <h2 style={{ margin: 0 }}>Connections</h2>
        <Link className="btn" href={`/workspaces/${id}/new-connection`}>
          + New connection
        </Link>
      </div>
      {connections.data?.length === 0 && (
        <p className="meta">
          No connections yet — add a source and a destination, then wire them
          together.
        </p>
      )}
      {connections.data?.map((c) => (
        <div className="card" key={c.connectionId}>
          <div className="row">
            <h3>
              <Link href={`/connections/${c.connectionId}`}>{c.name}</Link>
            </h3>
            <span>
              <span className="meta" style={{ marginRight: "0.75rem" }}>
                {c.catalog.streams.length} stream
                {c.catalog.streams.length === 1 ? "" : "s"}
              </span>
              <StatusBadge status={c.status} />
            </span>
          </div>
        </div>
      ))}
    </main>
  );
}

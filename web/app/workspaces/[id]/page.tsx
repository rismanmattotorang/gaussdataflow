"use client";

// Workspace: scoped health strip, pipelines with last-run status and
// one-click sync, sources/destinations with guarded deletes.

import Link from "next/link";
import { useParams } from "next/navigation";
import { useMemo, useState } from "react";
import { api, scheduleLabel } from "@/lib/api";
import {
  Breadcrumbs,
  ErrorNote,
  StatCard,
  StatusBadge,
  fmtNum,
  timeAgo,
  toast,
  usePoll,
} from "@/components/ui";

export default function WorkspacePage() {
  const { id } = useParams<{ id: string }>();
  const [query, setQuery] = useState("");

  const { data: workspace } = usePoll(() => api.workspaces.get(id), null, [id]);
  const stats = usePoll(() => api.stats(id), 5000, [id]);
  const sources = usePoll(() => api.actors("sources").list(id), 15000, [id]);
  const destinations = usePoll(
    () => api.actors("destinations").list(id),
    15000,
    [id],
  );
  const connections = usePoll(() => api.connections.list(id), 5000, [id]);
  const recentJobs = usePoll(() => api.jobs.recent(id, 50), 5000, [id]);

  const lastJob = useMemo(() => {
    const m = new Map<string, { status: string; createdAt: string }>();
    for (const j of recentJobs.data ?? []) {
      if (!m.has(j.connectionId))
        m.set(j.connectionId, { status: j.status, createdAt: j.createdAt });
    }
    return m;
  }, [recentJobs.data]);

  const filteredConnections = useMemo(
    () =>
      (connections.data ?? []).filter((c) =>
        c.name.toLowerCase().includes(query.toLowerCase()),
      ),
    [connections.data, query],
  );

  async function syncNow(connectionId: string, name: string) {
    try {
      const job = await api.connections.sync(connectionId);
      toast(`Sync for “${name}” queued as job #${job.id}`);
      recentJobs.refresh();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  }

  async function removeActor(
    kind: "sources" | "destinations",
    actorId: string,
    name: string,
  ) {
    if (
      !window.confirm(
        `Delete ${kind === "sources" ? "source" : "destination"} “${name}”? This cannot be undone.`,
      )
    )
      return;
    try {
      await api.actors(kind).remove(actorId);
      toast(`Deleted “${name}”`);
      (kind === "sources" ? sources : destinations).refresh();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  }

  const s = stats.data;
  return (
    <main>
      <Breadcrumbs
        items={[
          { label: "Mission control", href: "/" },
          { label: workspace?.name ?? "Workspace" },
        ]}
      />
      <h1>{workspace?.name ?? "Workspace"}</h1>
      <p className="lede">Sources, destinations, and the pipelines between them.</p>
      <ErrorNote
        error={sources.error ?? destinations.error ?? connections.error}
      />

      <div className="stat-grid">
        <StatCard
          label="Pipelines"
          value={s ? fmtNum(s.connections) : "…"}
          detail={
            s
              ? `${fmtNum(s.sources)} sources → ${fmtNum(s.destinations)} destinations`
              : undefined
          }
        />
        <StatCard
          label="In flight"
          value={s ? fmtNum(s.jobsRunning) : "…"}
          detail={s ? `${fmtNum(s.jobsPending)} pending` : undefined}
          tone={s && s.jobsRunning > 0 ? "warn" : undefined}
        />
        <StatCard
          label="Last 24 h"
          value={
            s ? `${fmtNum(s.jobsSucceeded24h)} ✓ · ${fmtNum(s.jobsFailed24h)} ✗` : "…"
          }
          detail={s ? `${fmtNum(s.recordsSynced24h)} records` : undefined}
          tone={
            s && s.jobsFailed24h > 0
              ? "err"
              : s && s.jobsSucceeded24h > 0
                ? "ok"
                : undefined
          }
        />
        <StatCard
          label="Last success"
          value={s?.lastSuccessAt ? timeAgo(s.lastSuccessAt) : "—"}
        />
      </div>

      <div
        className="form-row"
        style={{ justifyContent: "space-between", marginTop: "2rem" }}
      >
        <h2 style={{ margin: 0 }}>Connections</h2>
        <span className="form-row" style={{ margin: 0 }}>
          {(connections.data?.length ?? 0) > 5 && (
            <input
              placeholder="Filter…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ maxWidth: "12rem" }}
            />
          )}
          <Link className="btn" href={`/workspaces/${id}/new-connection`}>
            + New connection
          </Link>
        </span>
      </div>
      {connections.data?.length === 0 && (
        <p className="meta">
          No connections yet — add a source and a destination, then wire them
          together.
        </p>
      )}
      {filteredConnections.map((c) => {
        const last = lastJob.get(c.connectionId);
        const busy =
          last !== undefined &&
          (last.status === "pending" || last.status === "running");
        return (
          <div className="card" key={c.connectionId}>
            <div className="row">
              <h3>
                <Link href={`/connections/${c.connectionId}`}>{c.name}</Link>
              </h3>
              <span className="row" style={{ gap: "0.75rem" }}>
                <span className="meta">
                  {c.catalog.streams.length} stream
                  {c.catalog.streams.length === 1 ? "" : "s"} ·{" "}
                  {scheduleLabel(c.schedule)}
                </span>
                {last ? (
                  <span className="meta">
                    last run <StatusBadge status={last.status} />{" "}
                    {timeAgo(last.createdAt)}
                  </span>
                ) : (
                  <span className="meta">never ran</span>
                )}
                <StatusBadge status={c.status} />
                <button
                  className="ghost"
                  disabled={busy || c.status !== "active"}
                  onClick={() => syncNow(c.connectionId, c.name)}
                >
                  {busy ? "Running…" : "Sync now"}
                </button>
              </span>
            </div>
          </div>
        );
      })}

      <div className="grid-2" style={{ marginTop: "2rem" }}>
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
                <span className="row" style={{ gap: "0.75rem" }}>
                  <span className="meta">created {timeAgo(s.createdAt)}</span>
                  <button
                    className="danger"
                    onClick={() => removeActor("sources", s.id, s.name)}
                  >
                    Delete
                  </button>
                </span>
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
                <span className="row" style={{ gap: "0.75rem" }}>
                  <span className="meta">created {timeAgo(d.createdAt)}</span>
                  <button
                    className="danger"
                    onClick={() => removeActor("destinations", d.id, d.name)}
                  >
                    Delete
                  </button>
                </span>
              </div>
            </div>
          ))}
        </section>
      </div>
    </main>
  );
}

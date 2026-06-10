"use client";

// Mission control: fleet-wide health at a glance, live activity, and
// workspaces — the first screen answers "is my data moving?".

import Link from "next/link";
import { useMemo, useState } from "react";
import { api } from "@/lib/api";
import {
  ErrorNote,
  StatCard,
  StatusBadge,
  duration,
  fmtNum,
  timeAgo,
  toast,
  usePoll,
} from "@/components/ui";

export default function Dashboard() {
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);
  const [query, setQuery] = useState("");

  const { data: workspaces, error, refresh } = usePoll(
    api.workspaces.list,
    15000,
  );
  const stats = usePoll(() => api.stats(), 5000);
  const activity = usePoll(() => api.jobs.recent(undefined, 20), 5000);

  const filtered = useMemo(
    () =>
      (workspaces ?? []).filter((w) =>
        w.name.toLowerCase().includes(query.toLowerCase()),
      ),
    [workspaces, query],
  );

  async function create() {
    if (!name.trim()) return;
    setCreating(true);
    try {
      const ws = await api.workspaces.create(name.trim());
      toast(`Workspace “${ws.name}” created`);
      setName("");
      refresh();
    } catch (e) {
      toast((e as Error).message, "err");
    } finally {
      setCreating(false);
    }
  }

  const s = stats.data;
  return (
    <main>
      <h1>Mission control</h1>
      <p className="lede">
        Fleet-wide pulse of every pipeline: what&apos;s moving, what&apos;s
        queued, and what needs attention.
      </p>
      <ErrorNote error={error ?? stats.error} />

      <div className="stat-grid">
        <StatCard
          label="Pipelines"
          value={s ? fmtNum(s.connections) : "…"}
          detail={
            s ? `${fmtNum(s.sources)} sources → ${fmtNum(s.destinations)} destinations` : undefined
          }
        />
        <StatCard
          label="In flight"
          value={s ? fmtNum(s.jobsRunning) : "…"}
          detail={s ? `${fmtNum(s.jobsPending)} pending in queue` : undefined}
          tone={s && s.jobsRunning > 0 ? "warn" : undefined}
        />
        <StatCard
          label="Last 24 h"
          value={s ? `${fmtNum(s.jobsSucceeded24h)} ✓ · ${fmtNum(s.jobsFailed24h)} ✗` : "…"}
          detail={s ? `${fmtNum(s.recordsSynced24h)} records moved` : undefined}
          tone={s && s.jobsFailed24h > 0 ? "err" : "ok"}
        />
        <StatCard
          label="Last success"
          value={s?.lastSuccessAt ? timeAgo(s.lastSuccessAt) : "—"}
          detail={s?.lastSuccessAt ? undefined : "no successful sync yet"}
        />
      </div>

      <div className="grid-2" style={{ alignItems: "start", marginTop: "2rem" }}>
        <section>
          <div className="form-row" style={{ justifyContent: "space-between" }}>
            <h2 style={{ margin: 0 }}>Workspaces</h2>
          </div>
          <div className="form-row">
            <input
              placeholder="New workspace name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && create()}
              style={{ maxWidth: "14rem" }}
            />
            <button onClick={create} disabled={creating || !name.trim()}>
              Create
            </button>
          </div>
          {(workspaces?.length ?? 0) > 5 && (
            <input
              placeholder="Filter workspaces…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ marginBottom: "0.75rem" }}
            />
          )}
          {workspaces?.length === 0 && (
            <p className="meta">No workspaces yet — create your first one above.</p>
          )}
          {filtered.map((ws) => (
            <div className="card" key={ws.workspaceId}>
              <div className="row">
                <h3>
                  <Link href={`/workspaces/${ws.workspaceId}`}>{ws.name}</Link>
                </h3>
                <span className="meta">created {timeAgo(ws.createdAt)}</span>
              </div>
            </div>
          ))}
        </section>

        <section>
          <h2 style={{ margin: "0 0 0.75rem" }}>Live activity</h2>
          {activity.data?.length === 0 && (
            <p className="meta">No syncs yet — activity shows up here live.</p>
          )}
          {activity.data && activity.data.length > 0 && (
            <table>
              <thead>
                <tr>
                  <th>Job</th>
                  <th>Connection</th>
                  <th>Status</th>
                  <th>Records</th>
                  <th>When</th>
                </tr>
              </thead>
              <tbody>
                {activity.data.map((j) => (
                  <tr key={j.id}>
                    <td className="meta">#{j.id}</td>
                    <td>
                      <Link href={`/connections/${j.connectionId}`}>
                        {j.connectionName}
                      </Link>
                    </td>
                    <td>
                      <StatusBadge status={j.status} />
                      {j.status === "running" && (
                        <span className="meta"> {duration(j.startedAt)}</span>
                      )}
                    </td>
                    <td className="meta">{fmtNum(j.recordsSynced)}</td>
                    <td className="meta">{timeAgo(j.createdAt)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      </div>
    </main>
  );
}

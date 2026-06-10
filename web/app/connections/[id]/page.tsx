"use client";

// Connection detail: sync trigger, live job monitoring, committed state.

import { useState } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { api } from "@/lib/api";
import { ErrorNote, StatusBadge, timeAgo, usePoll } from "@/components/ui";

export default function ConnectionPage() {
  const { id } = useParams<{ id: string }>();
  const [actionError, setActionError] = useState<string | null>(null);

  const { data: connection, error } = usePoll(
    () => api.connections.get(id),
    null,
    [id],
  );
  // Jobs poll every 3s — a sync in flight updates live.
  const jobs = usePoll(() => api.connections.jobs(id), 3000, [id]);
  const state = usePoll(() => api.connections.state(id), 5000, [id]);

  async function act(fn: () => Promise<unknown>) {
    setActionError(null);
    try {
      await fn();
      jobs.refresh();
    } catch (e) {
      setActionError((e as Error).message);
    }
  }

  const hasActive = jobs.data?.some(
    (j) => j.status === "pending" || j.status === "running",
  );

  return (
    <main>
      <h1>{connection?.name ?? "Connection"}</h1>
      <p className="lede">
        {connection && (
          <>
            <StatusBadge status={connection.status} />{" "}
            <span className="meta">
              {connection.catalog.streams.length} stream
              {connection.catalog.streams.length === 1 ? "" : "s"} ·{" "}
              {connection.schedule
                ? `schedule ${JSON.stringify(connection.schedule)}`
                : "manual sync"}{" "}
              ·{" "}
              <Link href={`/workspaces/${connection.workspaceId}`}>
                back to workspace
              </Link>
            </span>
          </>
        )}
      </p>
      <ErrorNote error={error ?? jobs.error ?? actionError} />

      <div className="form-row">
        <button
          disabled={!connection || hasActive || connection.status !== "active"}
          onClick={() => act(() => api.connections.sync(id))}
        >
          Sync now
        </button>
        {hasActive && <span className="meta">a job is already queued/running</span>}
      </div>

      <h2>Streams</h2>
      <table>
        <thead>
          <tr>
            <th>Stream</th>
            <th>Sync mode</th>
            <th>Destination mode</th>
          </tr>
        </thead>
        <tbody>
          {connection?.catalog.streams.map((s) => (
            <tr key={s.stream.name}>
              <td>{s.stream.name}</td>
              <td>{s.sync_mode}</td>
              <td>{s.destination_sync_mode}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <h2>Jobs</h2>
      {jobs.data?.length === 0 && (
        <p className="meta">No jobs yet — trigger your first sync.</p>
      )}
      {jobs.data && jobs.data.length > 0 && (
        <table>
          <thead>
            <tr>
              <th>#</th>
              <th>Status</th>
              <th>Created</th>
              <th>Completed</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {jobs.data.map((job) => (
              <tr key={job.id}>
                <td>{job.id}</td>
                <td>
                  <StatusBadge status={job.status} />
                </td>
                <td className="meta">{timeAgo(job.createdAt)}</td>
                <td className="meta">{timeAgo(job.completedAt)}</td>
                <td>
                  {(job.status === "pending" || job.status === "running") && (
                    <button
                      className="danger"
                      onClick={() => act(() => api.jobs.cancel(job.id))}
                    >
                      Cancel
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h2>Committed state</h2>
      <p className="hint">
        Per-stream cursors acknowledged by the destination — what the next sync
        resumes from.
      </p>
      <pre>
        <code>
          {state.data?.state
            ? JSON.stringify(state.data.state, null, 2)
            : "No state yet — runs after the first successful incremental sync."}
        </code>
      </pre>
    </main>
  );
}

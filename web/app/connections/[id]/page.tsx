"use client";

// Connection detail: sync trigger, live job monitoring with attempt
// drill-down, committed state.

import { useState } from "react";
import { useParams } from "next/navigation";
import { api, scheduleLabel, type Job } from "@/lib/api";
import {
  Breadcrumbs,
  ErrorNote,
  StatusBadge,
  duration,
  fmtNum,
  timeAgo,
  toast,
  usePoll,
} from "@/components/ui";

function JobRow({ job, onCancel }: { job: Job; onCancel: () => void }) {
  const [detail, setDetail] = useState<Job | null>(null);
  const [open, setOpen] = useState(false);

  async function toggle() {
    if (!open) {
      try {
        setDetail(await api.jobs.get(job.id));
      } catch {
        setDetail(null);
      }
    }
    setOpen(!open);
  }

  const records = detail?.attempts?.length
    ? detail.attempts[detail.attempts.length - 1].recordsSynced
    : undefined;

  return (
    <>
      <tr onClick={toggle} style={{ cursor: "pointer" }}>
        <td className="meta">
          {open ? "▾" : "▸"} #{job.id}
        </td>
        <td>
          <StatusBadge status={job.status} />
        </td>
        <td className="meta">{duration(job.startedAt, job.completedAt)}</td>
        <td className="meta">{timeAgo(job.createdAt)}</td>
        <td className="meta">{timeAgo(job.completedAt)}</td>
        <td>
          {(job.status === "pending" || job.status === "running") && (
            <button
              className="danger"
              onClick={(e) => {
                e.stopPropagation();
                onCancel();
              }}
            >
              Cancel
            </button>
          )}
        </td>
      </tr>
      {open && (
        <tr className="attempt-row">
          <td colSpan={6}>
            {!detail?.attempts?.length ? (
              <span className="meta">No attempts recorded yet.</span>
            ) : (
              <table className="attempts">
                <thead>
                  <tr>
                    <th>Attempt</th>
                    <th>Status</th>
                    <th>Records</th>
                    <th>Duration</th>
                  </tr>
                </thead>
                <tbody>
                  {detail.attempts.map((a) => (
                    <tr key={a.id}>
                      <td className="meta">{a.attemptNumber}</td>
                      <td>
                        <StatusBadge status={a.status} />
                      </td>
                      <td className="meta">{fmtNum(a.recordsSynced)}</td>
                      <td className="meta">{duration(a.createdAt, a.endedAt)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {records !== undefined && (
              <p className="hint">
                Latest attempt moved {fmtNum(records)} records.
              </p>
            )}
          </td>
        </tr>
      )}
    </>
  );
}

export default function ConnectionPage() {
  const { id } = useParams<{ id: string }>();

  const { data: connection, error } = usePoll(
    () => api.connections.get(id),
    null,
    [id],
  );
  // Jobs poll every 3s — a sync in flight updates live.
  const jobs = usePoll(() => api.connections.jobs(id), 3000, [id]);
  const state = usePoll(() => api.connections.state(id), 5000, [id]);

  async function syncNow() {
    try {
      const job = await api.connections.sync(id);
      toast(`Sync queued as job #${job.id}`);
      jobs.refresh();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  }

  async function cancel(jobId: number) {
    try {
      await api.jobs.cancel(jobId);
      toast(`Cancellation requested for job #${jobId}`);
      jobs.refresh();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  }

  const hasActive = jobs.data?.some(
    (j) => j.status === "pending" || j.status === "running",
  );

  return (
    <main>
      <Breadcrumbs
        items={[
          { label: "Mission control", href: "/" },
          {
            label: "Workspace",
            href: connection ? `/workspaces/${connection.workspaceId}` : undefined,
          },
          { label: connection?.name ?? "Connection" },
        ]}
      />
      <h1>{connection?.name ?? "Connection"}</h1>
      <p className="lede">
        {connection && (
          <>
            <StatusBadge status={connection.status} />{" "}
            <span className="meta">
              {connection.catalog.streams.length} stream
              {connection.catalog.streams.length === 1 ? "" : "s"} ·{" "}
              {scheduleLabel(connection.schedule)}
            </span>
          </>
        )}
      </p>
      <ErrorNote error={error ?? jobs.error} />

      <div className="form-row">
        <button
          disabled={!connection || hasActive || connection.status !== "active"}
          onClick={syncNow}
        >
          Sync now
        </button>
        {hasActive && (
          <span className="meta">a job is already queued/running</span>
        )}
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
        <>
          <p className="hint">Click a job to see its attempt history.</p>
          <table>
            <thead>
              <tr>
                <th>#</th>
                <th>Status</th>
                <th>Duration</th>
                <th>Created</th>
                <th>Completed</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {jobs.data.map((job) => (
                <JobRow key={job.id} job={job} onCancel={() => cancel(job.id)} />
              ))}
            </tbody>
          </table>
        </>
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

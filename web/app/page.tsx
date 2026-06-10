"use client";

import Link from "next/link";
import { useState } from "react";
import { api } from "@/lib/api";
import { ErrorNote, timeAgo, usePoll } from "@/components/ui";

export default function Dashboard() {
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);
  const { data: workspaces, error, refresh } = usePoll(
    api.workspaces.list,
    null,
  );

  async function create() {
    if (!name.trim()) return;
    setCreating(true);
    try {
      await api.workspaces.create(name.trim());
      setName("");
      refresh();
    } finally {
      setCreating(false);
    }
  }

  return (
    <main>
      <h1>Workspaces</h1>
      <p className="lede">
        A workspace holds your sources, destinations, and the connections that
        move data between them.
      </p>
      <ErrorNote error={error} />

      <div className="form-row">
        <input
          placeholder="New workspace name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && create()}
          style={{ maxWidth: "20rem" }}
        />
        <button onClick={create} disabled={creating || !name.trim()}>
          Create workspace
        </button>
      </div>

      {workspaces?.length === 0 && (
        <p className="meta">No workspaces yet — create your first one above.</p>
      )}
      {workspaces?.map((ws) => (
        <div className="card" key={ws.workspaceId}>
          <div className="row">
            <h3>
              <Link href={`/workspaces/${ws.workspaceId}`}>{ws.name}</Link>
            </h3>
            <span className="meta">created {timeAgo(ws.createdAt)}</span>
          </div>
        </div>
      ))}
    </main>
  );
}

"use client";

// Connection builder: choose source + destination, discover the source's
// streams, pick streams and sync modes, set an optional schedule.

import { useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { api, ConfiguredStream, DiscoveredStream } from "@/lib/api";
import { Breadcrumbs, ErrorNote, toast, usePoll } from "@/components/ui";

interface StreamChoice {
  stream: DiscoveredStream;
  selected: boolean;
  syncMode: string;
}

export default function NewConnectionPage() {
  const { id: workspaceId } = useParams<{ id: string }>();
  const router = useRouter();

  const { data: workspace } = usePoll(
    () => api.workspaces.get(workspaceId),
    null,
    [workspaceId],
  );
  const sources = usePoll(() => api.actors("sources").list(workspaceId), null, [
    workspaceId,
  ]);
  const destinations = usePoll(
    () => api.actors("destinations").list(workspaceId),
    null,
    [workspaceId],
  );

  const [name, setName] = useState("");
  const [sourceId, setSourceId] = useState("");
  const [destinationId, setDestinationId] = useState("");
  const [streams, setStreams] = useState<StreamChoice[] | null>(null);
  const [scheduleKind, setScheduleKind] = useState<"manual" | "interval" | "cron">(
    "manual",
  );
  const [intervalMinutes, setIntervalMinutes] = useState(60);
  const [cron, setCron] = useState("0 * * * *");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function discover() {
    setBusy(true);
    setError(null);
    try {
      const catalog = await api.discover(sourceId);
      setStreams(
        catalog.streams.map((stream) => ({
          stream,
          selected: true,
          syncMode: stream.supported_sync_modes?.includes("incremental")
            ? "incremental"
            : "full_refresh",
        })),
      );
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  async function create() {
    setBusy(true);
    setError(null);
    try {
      const configured: ConfiguredStream[] = streams!
        .filter((c) => c.selected)
        .map((c) => ({
          stream: {
            name: c.stream.name,
            json_schema: c.stream.json_schema,
            supported_sync_modes: c.stream.supported_sync_modes,
          },
          sync_mode: c.syncMode,
          destination_sync_mode: "append",
          cursor_field:
            c.syncMode === "incremental"
              ? c.stream.default_cursor_field
              : undefined,
        }));
      const schedule =
        scheduleKind === "interval"
          ? { intervalMinutes }
          : scheduleKind === "cron"
            ? { cron }
            : undefined;
      const connection = await api.connections.create({
        name: name.trim() || "connection",
        sourceId,
        destinationId,
        catalog: { streams: configured },
        schedule,
      });
      toast(
        `Connection “${connection.name}” created — trigger your first sync`,
      );
      router.push(`/connections/${connection.connectionId}`);
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  }

  const ready =
    sourceId && destinationId && streams?.some((c) => c.selected) && !busy;

  const allSelected = streams?.every((c) => c.selected) ?? false;

  return (
    <main>
      <Breadcrumbs
        items={[
          { label: "Mission control", href: "/" },
          {
            label: workspace?.name ?? "Workspace",
            href: `/workspaces/${workspaceId}`,
          },
          { label: "New connection" },
        ]}
      />
      <h1>New connection</h1>
      <p className="lede">
        Wire a source to a destination. Discovery asks the source connector
        which streams it can replicate.
      </p>
      <ErrorNote error={sources.error ?? destinations.error ?? error} />

      <div className="grid-2">
        <div className="schema-field">
          <label>Source</label>
          <select
            value={sourceId}
            onChange={(e) => {
              setSourceId(e.target.value);
              setStreams(null);
            }}
          >
            <option value="">Select…</option>
            {sources.data?.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </div>
        <div className="schema-field">
          <label>Destination</label>
          <select
            value={destinationId}
            onChange={(e) => setDestinationId(e.target.value)}
          >
            <option value="">Select…</option>
            {destinations.data?.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="schema-field">
        <label>Connection name</label>
        <input
          value={name}
          placeholder="e.g. production-db → warehouse"
          onChange={(e) => setName(e.target.value)}
        />
      </div>

      {!streams && (
        <button onClick={discover} disabled={!sourceId || busy}>
          {busy ? "Discovering…" : "Discover streams"}
        </button>
      )}

      {streams && (
        <>
          <div className="form-row" style={{ justifyContent: "space-between" }}>
            <h2 style={{ margin: 0 }}>
              Streams ({streams.filter((c) => c.selected).length}/
              {streams.length} selected)
            </h2>
            <button className="ghost" onClick={discover} disabled={busy}>
              {busy ? "Discovering…" : "Re-discover"}
            </button>
          </div>
          <table>
            <thead>
              <tr>
                <th>
                  <input
                    type="checkbox"
                    checked={allSelected}
                    aria-label="Select all streams"
                    onChange={(e) =>
                      setStreams(
                        streams.map((c) => ({
                          ...c,
                          selected: e.target.checked,
                        })),
                      )
                    }
                  />
                </th>
                <th>Stream</th>
                <th>Sync mode</th>
              </tr>
            </thead>
            <tbody>
              {streams.map((choice, i) => (
                <tr key={choice.stream.name}>
                  <td>
                    <input
                      type="checkbox"
                      checked={choice.selected}
                      onChange={(e) =>
                        setStreams(
                          streams.map((c, j) =>
                            j === i ? { ...c, selected: e.target.checked } : c,
                          ),
                        )
                      }
                    />
                  </td>
                  <td>{choice.stream.name}</td>
                  <td>
                    <select
                      value={choice.syncMode}
                      onChange={(e) =>
                        setStreams(
                          streams.map((c, j) =>
                            j === i ? { ...c, syncMode: e.target.value } : c,
                          ),
                        )
                      }
                      style={{ maxWidth: "12rem" }}
                    >
                      {(
                        choice.stream.supported_sync_modes ?? ["full_refresh"]
                      ).map((mode) => (
                        <option key={mode} value={mode}>
                          {mode}
                        </option>
                      ))}
                    </select>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <h2>Schedule</h2>
          <div className="form-row">
            <select
              value={scheduleKind}
              onChange={(e) =>
                setScheduleKind(e.target.value as typeof scheduleKind)
              }
              style={{ maxWidth: "12rem" }}
            >
              <option value="manual">Manual only</option>
              <option value="interval">Every N minutes</option>
              <option value="cron">Cron</option>
            </select>
            {scheduleKind === "interval" && (
              <input
                type="number"
                min={1}
                value={intervalMinutes}
                onChange={(e) =>
                  setIntervalMinutes(parseInt(e.target.value || "60", 10))
                }
                style={{ maxWidth: "8rem" }}
              />
            )}
            {scheduleKind === "cron" && (
              <input
                value={cron}
                onChange={(e) => setCron(e.target.value)}
                style={{ maxWidth: "14rem" }}
              />
            )}
          </div>
          {scheduleKind === "cron" && (
            <p className="hint">
              Standard 5-field cron, evaluated in UTC — e.g.{" "}
              <code>0 * * * *</code> is hourly on the hour.
            </p>
          )}

          <div className="form-row">
            <button onClick={create} disabled={!ready}>
              Create connection
            </button>
          </div>
        </>
      )}
    </main>
  );
}

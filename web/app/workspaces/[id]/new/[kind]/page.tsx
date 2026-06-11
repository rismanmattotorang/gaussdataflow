"use client";

// Connector setup: pick a definition from the registry, fill in the
// spec-driven configuration form, test the connection, save.

import { useMemo, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { api, Definition } from "@/lib/api";
import SchemaForm, { ConfigValue } from "@/components/SchemaForm";
import { Breadcrumbs, ErrorNote, toast, usePoll } from "@/components/ui";

export default function NewActorPage() {
  const { id: workspaceId, kind } = useParams<{ id: string; kind: string }>();
  const router = useRouter();
  const isSource = kind === "source";
  const actors = api.actors(isSource ? "sources" : "destinations");

  const { data: workspace } = usePoll(
    () => api.workspaces.get(workspaceId),
    null,
    [workspaceId],
  );
  const { data: definitions, error: defError } = usePoll(
    () =>
      isSource ? api.definitions.sources() : api.definitions.destinations(),
    null,
    [kind],
  );

  const [definitionId, setDefinitionId] = useState("");
  const [name, setName] = useState("");
  const [config, setConfig] = useState<ConfigValue>({});
  const [busy, setBusy] = useState<"test" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checkResult, setCheckResult] = useState<string | null>(null);

  const definition: Definition | undefined = useMemo(
    () => definitions?.find((d) => d.definitionId === definitionId),
    [definitions, definitionId],
  );
  const schema = definition?.spec?.connectionSpecification as
    | Record<string, unknown>
    | undefined;

  async function save(check: boolean) {
    setBusy(check ? "test" : "save");
    setError(null);
    setCheckResult(null);
    const label = name.trim() || definition!.name;
    try {
      const actor = await actors.create({
        name: label,
        workspaceId,
        definitionId,
        configuration: config,
      });
      if (check) {
        const result = await actors.check(actor.id);
        if (result.status !== "SUCCEEDED") {
          setCheckResult(
            `Connection test failed: ${result.message ?? "unknown error"}. Nothing was saved — fix the configuration and try again.`,
          );
          await actors.remove(actor.id);
          setBusy(null);
          return;
        }
      }
      toast(
        check
          ? `${isSource ? "Source" : "Destination"} “${label}” tested and saved`
          : `${isSource ? "Source" : "Destination"} “${label}” saved (untested)`,
      );
      router.push(`/workspaces/${workspaceId}`);
    } catch (e) {
      setError((e as Error).message);
      setBusy(null);
    }
  }

  return (
    <main>
      <Breadcrumbs
        items={[
          { label: "Mission control", href: "/" },
          {
            label: workspace?.name ?? "Workspace",
            href: `/workspaces/${workspaceId}`,
          },
          { label: `New ${isSource ? "source" : "destination"}` },
        ]}
      />
      <h1>New {isSource ? "source" : "destination"}</h1>
      <p className="lede">
        Pick a connector from the registry and configure it. Fields marked
        secret are sealed into the secrets backend and never shown again.
      </p>
      <ErrorNote error={defError ?? error} />
      {checkResult && <p className="error-note">{checkResult}</p>}

      {definitions?.length === 0 && (
        <p className="meta">
          No {isSource ? "source" : "destination"} connectors are registered
          yet. Seed the registry (<code>--seed-registry</code>) or import one
          via <code>POST /api/v1/definitions/import</code>, then reload this
          page.
        </p>
      )}

      <div className="schema-field">
        <label>Connector</label>
        <select
          value={definitionId}
          onChange={(e) => {
            setDefinitionId(e.target.value);
            setConfig({});
            setCheckResult(null);
          }}
        >
          <option value="">Select a connector…</option>
          {definitions?.map((d) => (
            <option key={d.definitionId} value={d.definitionId}>
              {d.name} ({d.dockerRepository}:{d.dockerImageTag})
            </option>
          ))}
        </select>
      </div>

      {definition && (
        <>
          <div className="schema-field">
            <label>Name</label>
            <input
              value={name}
              placeholder={definition.name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          <h2>Configuration</h2>
          <SchemaForm schema={schema} value={config} onChange={setConfig} />

          <div className="form-row">
            <button disabled={busy !== null} onClick={() => save(true)}>
              {busy === "test" ? "Testing connection…" : "Test & save"}
            </button>
            <button
              className="ghost"
              disabled={busy !== null}
              onClick={() => save(false)}
            >
              {busy === "save" ? "Saving…" : "Save without testing"}
            </button>
          </div>
          <p className="hint">
            “Test &amp; save” runs the connector&apos;s check operation against
            your configuration before keeping it.
          </p>
        </>
      )}
    </main>
  );
}

"use client";

// Connector setup: pick a definition from the registry, fill in the
// spec-driven configuration form, test the connection, save.

import { useMemo, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { api, Definition } from "@/lib/api";
import SchemaForm, { ConfigValue } from "@/components/SchemaForm";
import { ErrorNote, usePoll } from "@/components/ui";

export default function NewActorPage() {
  const { id: workspaceId, kind } = useParams<{ id: string; kind: string }>();
  const router = useRouter();
  const isSource = kind === "source";
  const actors = api.actors(isSource ? "sources" : "destinations");

  const { data: definitions, error: defError } = usePoll(
    () =>
      isSource ? api.definitions.sources() : api.definitions.destinations(),
    null,
    [kind],
  );

  const [definitionId, setDefinitionId] = useState("");
  const [name, setName] = useState("");
  const [config, setConfig] = useState<ConfigValue>({});
  const [busy, setBusy] = useState(false);
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
    setBusy(true);
    setError(null);
    setCheckResult(null);
    try {
      const actor = await actors.create({
        name: name.trim() || definition!.name,
        workspaceId,
        definitionId,
        configuration: config,
      });
      if (check) {
        const result = await actors.check(actor.id);
        setCheckResult(
          result.status === "SUCCEEDED"
            ? "Connection test succeeded."
            : `Connection test failed: ${result.message ?? "unknown error"}`,
        );
        if (result.status !== "SUCCEEDED") {
          await actors.remove(actor.id);
          setBusy(false);
          return;
        }
      }
      router.push(`/workspaces/${workspaceId}`);
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  }

  return (
    <main>
      <h1>New {isSource ? "source" : "destination"}</h1>
      <p className="lede">
        Pick a connector from the registry and configure it. Fields marked
        secret are sealed into the secrets backend and never shown again.
      </p>
      <ErrorNote error={defError ?? error} />
      {checkResult && (
        <p className={checkResult.includes("succeeded") ? "meta" : "error-note"}>
          {checkResult}
        </p>
      )}

      <div className="schema-field">
        <label>Connector</label>
        <select
          value={definitionId}
          onChange={(e) => {
            setDefinitionId(e.target.value);
            setConfig({});
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
            <button disabled={busy} onClick={() => save(true)}>
              Test &amp; save
            </button>
            <button className="ghost" disabled={busy} onClick={() => save(false)}>
              Save without testing
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

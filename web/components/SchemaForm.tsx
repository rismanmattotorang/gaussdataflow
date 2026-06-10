"use client";

// Renders a connector configuration form from its connectionSpecification
// JSON Schema — the heart of the connector setup experience. Supports
// string/number/integer/boolean/enum fields, nested objects, defaults,
// required markers, and `airbyte_secret` (rendered as password inputs).

import { useMemo } from "react";

type Schema = {
  type?: string;
  title?: string;
  description?: string;
  default?: unknown;
  enum?: unknown[];
  airbyte_secret?: boolean;
  properties?: Record<string, Schema>;
  required?: string[];
  order?: number;
};

export type ConfigValue = Record<string, unknown>;

function fieldOrder(props: Record<string, Schema>): string[] {
  return Object.keys(props).sort((a, b) => {
    const oa = props[a].order ?? 1000;
    const ob = props[b].order ?? 1000;
    return oa === ob ? a.localeCompare(b) : oa - ob;
  });
}

function defaultsFor(schema: Schema): ConfigValue {
  const out: ConfigValue = {};
  for (const [key, prop] of Object.entries(schema.properties ?? {})) {
    if (prop.default !== undefined) out[key] = prop.default;
    else if (prop.type === "object" && prop.properties) {
      const nested = defaultsFor(prop);
      if (Object.keys(nested).length > 0) out[key] = nested;
    }
  }
  return out;
}

export function useSchemaDefaults(schema: Schema | undefined): ConfigValue {
  return useMemo(() => (schema ? defaultsFor(schema) : {}), [schema]);
}

function Field({
  name,
  schema,
  required,
  value,
  onChange,
}: {
  name: string;
  schema: Schema;
  required: boolean;
  value: unknown;
  onChange: (v: unknown) => void;
}) {
  const label = schema.title ?? name;
  const id = `field-${name}`;

  if (schema.type === "object" && schema.properties) {
    const objValue = (value as ConfigValue) ?? {};
    return (
      <fieldset className="schema-group">
        <legend>{label}</legend>
        {schema.description && <p className="hint">{schema.description}</p>}
        {fieldOrder(schema.properties).map((key) => (
          <Field
            key={key}
            name={key}
            schema={schema.properties![key]}
            required={schema.required?.includes(key) ?? false}
            value={objValue[key]}
            onChange={(v) => onChange({ ...objValue, [key]: v })}
          />
        ))}
      </fieldset>
    );
  }

  let input: React.ReactNode;
  if (schema.type === "boolean") {
    input = (
      <input
        id={id}
        type="checkbox"
        checked={Boolean(value)}
        onChange={(e) => onChange(e.target.checked)}
      />
    );
  } else if (schema.enum) {
    input = (
      <select
        id={id}
        value={String(value ?? "")}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">—</option>
        {schema.enum.map((opt) => (
          <option key={String(opt)} value={String(opt)}>
            {String(opt)}
          </option>
        ))}
      </select>
    );
  } else if (schema.type === "integer" || schema.type === "number") {
    input = (
      <input
        id={id}
        type="number"
        value={value === undefined || value === null ? "" : Number(value)}
        onChange={(e) =>
          onChange(
            e.target.value === ""
              ? undefined
              : schema.type === "integer"
                ? parseInt(e.target.value, 10)
                : parseFloat(e.target.value),
          )
        }
      />
    );
  } else {
    input = (
      <input
        id={id}
        type={schema.airbyte_secret ? "password" : "text"}
        autoComplete={schema.airbyte_secret ? "new-password" : "off"}
        value={String(value ?? "")}
        onChange={(e) =>
          onChange(e.target.value === "" ? undefined : e.target.value)
        }
        placeholder={schema.airbyte_secret ? "••••••••" : undefined}
      />
    );
  }

  return (
    <div className="schema-field">
      <label htmlFor={id}>
        {label}
        {required && <span className="req">*</span>}
        {schema.airbyte_secret && <span className="badge later">secret</span>}
      </label>
      {input}
      {schema.description && <p className="hint">{schema.description}</p>}
    </div>
  );
}

export default function SchemaForm({
  schema,
  value,
  onChange,
}: {
  schema: Schema | undefined;
  value: ConfigValue;
  onChange: (v: ConfigValue) => void;
}) {
  if (!schema?.properties || Object.keys(schema.properties).length === 0) {
    return (
      <p className="hint">
        This connector has no published configuration spec — provide raw JSON
        below if needed.
      </p>
    );
  }
  return (
    <div>
      {fieldOrder(schema.properties).map((key) => (
        <Field
          key={key}
          name={key}
          schema={schema.properties![key]}
          required={schema.required?.includes(key) ?? false}
          value={value[key]}
          onChange={(v) => {
            const next = { ...value };
            if (v === undefined) delete next[key];
            else next[key] = v;
            onChange(next);
          }}
        />
      ))}
    </div>
  );
}

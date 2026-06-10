// Typed client for the gaussdataflow config API.

export const API_BASE =
  process.env.NEXT_PUBLIC_GAUSS_API ?? "http://127.0.0.1:8000";

export interface Workspace {
  workspaceId: string;
  name: string;
  createdAt: string;
}

export interface Definition {
  definitionId: string;
  actorType: "source" | "destination";
  name: string;
  dockerRepository: string;
  dockerImageTag: string;
  documentationUrl?: string;
  spec?: { connectionSpecification?: Record<string, unknown> };
}

export interface Actor {
  id: string;
  workspaceId: string;
  definitionId: string;
  actorType: "source" | "destination";
  name: string;
  configuration: Record<string, unknown>;
  createdAt: string;
}

export interface Connection {
  connectionId: string;
  workspaceId: string;
  sourceId: string;
  destinationId: string;
  name: string;
  status: string;
  catalog: { streams: ConfiguredStream[] };
  schedule?: Record<string, unknown> | null;
}

export interface ConfiguredStream {
  stream: { name: string; json_schema: unknown; supported_sync_modes?: string[] };
  sync_mode: string;
  destination_sync_mode: string;
  cursor_field?: string[];
}

export interface DiscoveredStream {
  name: string;
  json_schema: unknown;
  supported_sync_modes?: string[];
  default_cursor_field?: string[];
}

export interface Job {
  id: number;
  connectionId: string;
  jobType: string;
  status: string;
  createdAt: string;
  completedAt?: string;
  attempts?: Attempt[];
}

export interface Attempt {
  id: number;
  attemptNumber: number;
  status: string;
  recordsSynced?: number;
  createdAt: string;
  endedAt?: string;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
    cache: "no-store",
  });
  if (res.status === 204) return undefined as T;
  const body = await res.json().catch(() => null);
  if (!res.ok) {
    throw new Error(body?.message ?? `${res.status} ${res.statusText}`);
  }
  return body as T;
}

const list = <T,>(path: string) =>
  request<{ data: T[] }>(path).then((r) => r.data);

export const api = {
  workspaces: {
    list: () => list<Workspace>("/api/v1/workspaces"),
    get: (id: string) => request<Workspace>(`/api/v1/workspaces/${id}`),
    create: (name: string) =>
      request<Workspace>("/api/v1/workspaces", {
        method: "POST",
        body: JSON.stringify({ name }),
      }),
  },
  definitions: {
    sources: () => list<Definition>("/api/v1/source_definitions"),
    destinations: () => list<Definition>("/api/v1/destination_definitions"),
    get: (id: string) => request<Definition>(`/api/v1/definitions/${id}`),
  },
  actors: (kind: "sources" | "destinations") => ({
    list: (workspaceId: string) =>
      list<Actor>(`/api/v1/${kind}?workspaceId=${workspaceId}`),
    create: (body: {
      name: string;
      workspaceId: string;
      definitionId: string;
      configuration: Record<string, unknown>;
    }) =>
      request<Actor>(`/api/v1/${kind}`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
    remove: (id: string) =>
      request<void>(`/api/v1/${kind}/${id}`, { method: "DELETE" }),
    check: (id: string) =>
      request<{ status: string; message?: string }>(
        `/api/v1/${kind}/${id}/check`,
        { method: "POST" },
      ),
  }),
  discover: (sourceId: string) =>
    request<{ streams: DiscoveredStream[] }>(
      `/api/v1/sources/${sourceId}/discover`,
      { method: "POST" },
    ),
  connections: {
    list: (workspaceId: string) =>
      list<Connection>(`/api/v1/connections?workspaceId=${workspaceId}`),
    get: (id: string) => request<Connection>(`/api/v1/connections/${id}`),
    create: (body: {
      name: string;
      sourceId: string;
      destinationId: string;
      catalog: { streams: ConfiguredStream[] };
      schedule?: Record<string, unknown>;
    }) =>
      request<Connection>("/api/v1/connections", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    update: (id: string, patch: Record<string, unknown>) =>
      request<Connection>(`/api/v1/connections/${id}`, {
        method: "PATCH",
        body: JSON.stringify(patch),
      }),
    remove: (id: string) =>
      request<void>(`/api/v1/connections/${id}`, { method: "DELETE" }),
    sync: (id: string) =>
      request<Job>(`/api/v1/connections/${id}/sync`, { method: "POST" }),
    jobs: (id: string) => list<Job>(`/api/v1/connections/${id}/jobs`),
    state: (id: string) =>
      request<{ state: unknown }>(`/api/v1/connections/${id}/state`),
  },
  jobs: {
    get: (id: number) => request<Job>(`/api/v1/jobs/${id}`),
    cancel: (id: number) =>
      request<Job>(`/api/v1/jobs/${id}/cancel`, { method: "POST" }),
  },
};

# Gauss-DataFlow Roadmap — MCP Gateway & Agentic AI

**Maintained by Gaussian Technologies · MIT licensed, open source forever.**

Phases 0–6 of the founding roadmap have shipped (see
[ARCHITECTURE.md](ARCHITECTURE.md)), followed by **Phase 7 — operator
experience**: the Ratatui terminal console (`gauss-tui`), the mission-control
web dashboard, fleet observability endpoints (`GET /api/v1/stats`,
`GET /api/v1/jobs`), and an MCP gateway upgraded to negotiate protocol
revisions through `2025-06-18` with tool annotations, structured tool output,
and fleet-observability tools.

This document is the forward roadmap. It is grounded in a mid-2026 survey of
the MCP specification, production MCP gateway architectures, and how the data
integration industry is integrating AI agents. Sources are linked throughout.

---

## Where the state of the art is (research summary)

**The MCP specification has moved fast.** Since the `2024-11-05` baseline,
the spec added an OAuth 2.1 authorization framework, the streamable HTTP
transport, and tool annotations
([2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26/changelog)),
then structured tool output, elicitation, resource links, and
resource-server classification with RFC 8707 resource indicators
([2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/changelog)),
then OpenID Connect discovery, Client ID Metadata Documents, sampling with
tools, and experimental long-running **tasks**
([2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/changelog)).
The [2026-07-28 release candidate](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)
makes the protocol core stateless (plain load balancers work), introduces an
extensions framework, and graduates tasks. MCP itself is now governed by the
Linux Foundation's
[Agentic AI Foundation](https://www.anthropic.com/news/donating-the-model-context-protocol-and-establishing-of-the-agentic-ai-foundation),
and the official [MCP Registry](https://github.com/modelcontextprotocol/registry)
(preview) provides namespace-verified server discovery via `server.json`.

**Gateways have converged on a pattern set.** Production MCP gateways —
[IBM ContextForge](https://github.com/IBM/mcp-context-forge),
[agentgateway](https://github.com/agentgateway/agentgateway) (Rust),
[ToolHive](https://github.com/stacklok/toolhive),
[Docker MCP Gateway](https://github.com/docker/mcp-gateway),
[Microsoft mcp-gateway](https://github.com/microsoft/mcp-gateway) — share:
tool federation behind one endpoint, **virtual servers** (curated per-consumer
tool subsets), centralized OAuth with per-backend credential injection (never
token passthrough), per-client/per-tool rate limiting, full audit trails,
semantic tool search to fight tool-count context explosion, session-affinity
routing for streamable HTTP, and inline guardrails (PII redaction, prompt
injection scanning, tool-description pinning against rug-pulls — see the
official [security best practices](https://modelcontextprotocol.io/specification/draft/basic/security_best_practices)).

**Data platforms are racing to be agent-native.**
[Airbyte Agents](https://www.businesswire.com/news/home/20260505801702/en/Airbyte-Agents-Launched-to-Fix-the-Data-Problem-Breaking-AI-Agents)
ships a context layer over MCP; Airbyte's
[AI Assistant](https://docs.airbyte.com/platform/2.1/connector-development/connector-builder-ui/ai-assist)
prefills connector builders from API docs.
[Fivetran + dbt](https://www.fivetran.com/press/fivetran-dbt-labs-complete-merger-to-create-the-data-infrastructure-for-trusted-ai-agents)
push an "Agents Schema" standard. [dltHub](https://dlthub.com/blog/ai-workbench)
reports ~91% of new pipelines authored by coding agents through guarded,
skill-based workflows. [Dagster](https://docs.dagster.io/getting-started/ai-tools)
gives agents deterministic, validated CLI actions plus GitOps human review.
The recurring patterns: **AI connector builders**, **NL pipeline authoring**,
**agent-operated control planes**, **context layers**, and (still mostly
aspirational industry-wide) **self-healing pipelines**.

Gauss-DataFlow's edge: the entire control plane is already a typed Rust API
with sealed secrets and a Postgres-native queue, the connector layer has a
**declarative YAML engine** that is the ideal compile target for an AI
connector builder, and the MCP gateway is first-party, not bolted on.

---

## Phase 8 — MCP gateway: transport, identity, and parity *(up next)*

The gateway currently speaks stdio and talks to Postgres directly — which
means it **bypasses the REST control plane's RBAC tokens and audit log**.
Closing that gap is the first priority: agents must be subject to the same
governance as humans.

1. **Route the gateway through the control plane.** Every MCP tool call maps
   to the identical store operation as its REST twin and is recorded in the
   audit log with an agent subject (`mcp:<token-name>`). One policy layer for
   humans and agents.
2. **Streamable HTTP transport.** Serve MCP from `gauss-server` itself at
   `/mcp` (axum is already there), so remote agents — Claude custom
   connectors, ChatGPT developer mode, IDE clients — can connect without a
   local binary. Adopt the official Rust SDK
   ([rmcp](https://github.com/modelcontextprotocol/rust-sdk), which
   implements `2025-11-25` server-side streamable HTTP) or extend the
   hand-rolled core. Design for the
   [2026-07-28 stateless core](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)
   from day one: no session pinning required.
3. **OAuth 2.1 resource server.** Implement RFC 9728 protected-resource
   metadata and RFC 8707 resource indicators per the
   [authorization spec](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization),
   accepting Client ID Metadata Documents. Map OAuth scopes onto the existing
   viewer/editor/admin roles; bearer `gauss_…` tokens remain for
   first-party clients.
4. **Spec catch-up to `2025-11-25`.** Output schemas for every tool
   (structured content is already emitted), tool icons and titles,
   completions for argument autofill (workspace and connection ids), and
   pollable long-running **tasks** so `trigger_sync` can return a task handle
   the agent polls instead of busy-looping `get_job`.
5. **Elicitation for safe interactivity.** When an agent omits a required
   secret or a destructive action needs confirmation, ask the user through
   the client instead of failing
   ([elicitation](https://modelcontextprotocol.io/specification/2025-06-18/client/elicitation)).

## Phase 9 — Gateway governance: virtual servers, limits, registry

Adopt the patterns production gateways converged on, scoped to one platform:

1. **Virtual tool surfaces per role.** A viewer token's `tools/list` shows
   only read-only tools (the annotations shipped in Phase 7 are the policy
   input); editor adds mutations; admin adds governance. Least-privilege
   tool scoping is the top recommendation of every gateway survey and of
   [Anthropic's tool-design guidance](https://www.anthropic.com/engineering/writing-tools-for-agents).
2. **Rate limiting and quotas** per token and per tool class (connector
   launches like `check`/`discover` are expensive; cap them independently of
   cheap reads).
3. **Guardrails on tool results.** Secret-reference scrubbing is already
   structural (the envelope never exposes raw values); add PII-pattern
   redaction and size caps on records previewed through agent-facing tools.
4. **Publish to the MCP Registry.** Ship a `server.json` so
   `io.github.rismanmattotorang/gaussdataflow` is discoverable by clients and
   private sub-registries
   ([registry](https://github.com/modelcontextprotocol/registry)).
5. **Tool-description integrity.** Version and hash the tool catalog so
   clients can pin it — the defense against rug-pull description swaps
   highlighted in
   [MCP security research](https://modelcontextprotocol.io/specification/draft/basic/security_best_practices).

## Phase 10 — Agentic data engineering

Make agents productive *authors and operators* of pipelines, not just
button-pushers:

1. **AI connector builder targeting the declarative engine.** An agent (or
   the `connector_builder` MCP toolset) reads API docs and emits a
   `gauss-declarative` YAML manifest — auth, pagination, incremental cursors
   — then validates it with `check`/`discover`/sampled `read` before
   registering. The YAML manifest is a far safer compile target than
   generated code: it is reviewable, diffable, and sandboxed by construction.
   (Airbyte's [AI Assistant](https://docs.airbyte.com/platform/2.1/connector-development/connector-builder-ui/ai-assist)
   and Fivetran's
   [NL connector flow](https://www.fivetran.com/blog/integrate-data-faster-using-natural-language-fivetran-and-mcp)
   validate the demand; dltHub's
   [91% agent-authored pipelines](https://dlthub.com/blog/llm-native-data-engineering-accessible-for-all-python-developers)
   validate the trajectory.)
2. **Failure triage and self-healing loop.** On terminal job failure, an
   optional triage agent gets the trace messages, attempt history, and
   connector spec, and proposes a classified diagnosis (auth expired, schema
   drift, rate limit, transient) plus a remediation — re-run, adjust
   schedule, refresh OAuth token, or open a human ticket. Remediations that
   mutate state go through human approval first (elicitation or webhook).
   No major vendor verifiably ships full auto-repair yet; a guarded loop
   over our typed failure taxonomy is a differentiator.
3. **Schema-drift handling.** Persist the discovered catalog hash per
   connection; on drift, surface a diff in the console/TUI and let policy
   decide: auto-propagate additive changes, quarantine breaking ones,
   notify via webhook.
4. **Context layer for agents.** A read-optimized set of MCP
   resources/tools that answer "what data do we have, how fresh is it,
   what's its lineage" — the
   [Airbyte Agents](https://www.businesswire.com/news/home/20260505801702/en/Airbyte-Agents-Launched-to-Fix-the-Data-Problem-Breaking-AI-Agents)
   / [dltHub Scale](https://dlthub.com/products/dlthub) positioning — built
   from catalogs, committed state, and job history we already store.

## Phase 11 — Embedded agents, evals, and trust

1. **First-party operations agent.** Embed an agent runtime (Claude Agent
   SDK pattern: in-process MCP tools, `canUseTool` approval callbacks,
   hooks — see
   [building effective agents](https://www.anthropic.com/engineering/building-effective-agents))
   behind `gauss agent`, with the gateway's scoped tools as its only
   capabilities and OS-level sandboxing for connector execution
   ([sandbox-runtime](https://github.com/anthropic-experimental/sandbox-runtime)).
2. **Human-in-the-loop as a platform primitive.** A pending-approval queue in
   Postgres: any agent-initiated destructive mutation (delete, catalog
   replace, schedule change) parks until approved in the console/TUI —
   the same pattern as LangGraph `interrupt()` and OpenAI's
   `require_approval`, but enforced server-side where it can't be skipped.
3. **Agent evals in CI.** A scripted MCP client replays golden agent
   trajectories (configure → discover → wire → sync → verify state) against
   a hermetic platform with the mock connector; regressions in tool
   ergonomics fail CI. Trajectory-level evaluation is the emerging standard
   ([agentevals](https://github.com/langchain-ai/agentevals),
   [eval-driven tool iteration](https://www.anthropic.com/engineering/writing-tools-for-agents)).
4. **Token-efficient tool surface.** Keep the catalog small and
   high-leverage; if it grows past ~25 tools, add a `search_tools` meta-tool
   (semantic routing) rather than flooding context — the documented failure
   mode that gateway vendors and
   [Anthropic's advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use)
   both address.

---

## Sequencing and principles

| Order | Theme | Why first |
|---|---|---|
| 8 | Transport, identity, parity | Governance gap (gateway bypasses RBAC/audit) is a real risk today; remote transport unlocks every hosted client |
| 9 | Virtual servers, limits, registry | Cheap once 8 lands; converts annotations into enforced policy |
| 10 | Agentic authoring & healing | Highest differentiation; depends on a governed gateway |
| 11 | Embedded agents & evals | Productizes 10 with trust machinery |

Principles, in priority order: **agents get the same governance as humans;
secrets stay sealed; destructive actions need a human; everything stays MIT.**

---
name: agent-client-protocol
description: Work on Agent Client Protocol (ACP) integrations, including JSON-RPC methods, capabilities, sessions, streaming, tools, permissions, terminals, MCP wiring, transports, and editor/IDE clients.
---

# Agent Client Protocol

Implement ACP as a strict JSON-RPC 2.0 protocol between an agent and a client. Preserve ACP naming, use absolute file paths everywhere, treat line numbers as 1-based, and keep user-visible text in Markdown unless the target implementation already constrains formatting more tightly.

## Start

- Determine whether the task is agent-side, client-side, or both.
- If the project uses the npm package `@agentclientprotocol/sdk`, check the TypeScript SDK changelog at <https://raw.githubusercontent.com/agentclientprotocol/typescript-sdk/refs/heads/main/CHANGELOG.md> for SDK-specific changes before changing or validating SDK usage.
- Read [references/protocol-overview.md](references/protocol-overview.md) first for the lifecycle and required invariants.
- Read [references/content-tools-and-io.md](references/content-tools-and-io.md) when the task involves prompt content, tool calls, permissions, filesystem access, terminals, or MCP server wiring.
- Read [references/session-features-and-extensions.md](references/session-features-and-extensions.md) when the task involves session listing/loading, plans, modes, config options, slash commands, `_meta`, custom methods, or schema lookups.

## Follow This Workflow

1. Identify the capability gate before writing code.
   Check which side advertises the relevant capability during `initialize`, and do not call optional methods unless that capability is present.
2. Map the feature to the exact ACP surface.
   Decide which methods are requests, which are notifications, what each side sends, and what the success or error response must look like.
3. Preserve baseline guarantees.
   Support `session/new`, `session/prompt`, `session/cancel`, and `session/update` on every agent implementation. Support text and `resource_link` prompt content as the prompt baseline.
4. Implement the full lifecycle, not only the happy path.
   Handle initialization, authentication if required, prompt execution, streaming updates, cancellation, final response, and any required cleanup such as terminal release.
5. Treat optional features as additive.
   Add `authenticate`, `logout`, `session/load`, `session/resume`, `session/close`, `session/list`, HTTP/SSE MCP transports, image/audio/resource prompt blocks, modes, or config options only when the implementation advertises them.
6. Preserve extensibility correctly.
   Put custom data in `_meta`, keep custom method names prefixed with `_`, and avoid inventing new top-level ACP fields.
7. Validate cross-side behavior.
   Confirm the client and agent agree on capabilities, prompt content types, session update variants, and stop reasons.

## Core Rules

- Use absolute paths for all ACP file references.
- Use 1-based lines when sending or interpreting file locations.
- Treat omitted capabilities as unsupported.
- Prefer embedded `resource` content over `resource_link` when the client already has the bytes and the agent supports embedded context.
- Stream progress through `session/update`; do not hide long-running tool or terminal work behind a single final response.
- Return semantically correct stop reasons such as `end_turn` or `cancelled`.
- Continue accepting or sending final tool updates during cancellation until the original `session/prompt` request is resolved.
- Preserve `_meta` fields and reserved tracing keys rather than stripping them.

## Feature Map

- Use [references/protocol-overview.md](references/protocol-overview.md) for `initialize`, authentication, `session/new`, `session/prompt`, `session/cancel`, `session/load`, `session/resume`, `session/close`, `session/list`, stop reasons, and transport rules.
- Use [references/content-tools-and-io.md](references/content-tools-and-io.md) for `ContentBlock` variants, tool call creation and updates, permission requests, `fs/*`, `terminal/*`, and MCP server transport config.
- Use [references/session-features-and-extensions.md](references/session-features-and-extensions.md) for plans, modes, config options, slash commands, `_meta`, custom methods, and schema-level method/type names.

## Implementation Guidance

- Keep request and notification payloads close to the ACP schema names so tracing and debugging stay obvious.
- When reviewing an ACP implementation, check capability negotiation first; many bugs are really "optional feature called without being advertised".
- When adding client support, make the UI tolerate partial capability sets and unknown future fields.
- When adding agent support, send complete replacement payloads for plan updates and config option updates where ACP requires full-state replacement.
- When implementing stdio transport, write only newline-delimited ACP JSON-RPC messages to stdout and use stderr only for logs.

# Replace Goddard ACP Client Logic With `acp-client`

## Goal

Replace Goddard's local ACP client transport, process launch, and registry helper logic with direct use of the standalone `acp-client` npm package while preserving daemon session behavior, persisted transcript semantics, and app-visible IPC contracts.

## Current State

- Goddard uses `@agentclientprotocol/sdk` directly for protocol types and `ClientSideConnection` in daemon ACP client code today.
- `core/daemon/src/session/acp.ts` implements local low-level client helpers:
  - `createAgentMessageStream`;
  - `createAgentConnection`;
  - `isAcpRequest`;
  - `matchAcpRequest`;
  - `getAcpMessageResult`.
- `core/daemon/src/session/manager.ts` depends on those helpers for:
  - live session activation;
  - prompt completion settlement;
  - permission request detection;
  - launch preview inspection;
  - one-off raw stream setup.
- `core/daemon/src/session/inspect.ts` starts raw adapter inspections by combining Goddard's process launcher with the local message stream helper.
- Goddard has local ACP registry and agent-process support under `core/daemon/src/session/`, including registry fallback generation and agent binary/cache handling.
- `scripts/acp-session.ts` is a debugging CLI layered on the daemon and raw adapter inspection APIs.
- `acp-client@0.1.1` already exports:
  - all `@agentclientprotocol/sdk` types under the `acp` namespace;
  - the same low-level helpers listed above;
  - `createAcpClient` for host-owned streams;
  - `createNodeAcpClient` for registry-backed stdio launch;
  - registry, process, binary archive, filesystem, and terminal client helpers.

## Non-Goals

- Do not replace all workspace uses of `@agentclientprotocol/sdk`. App, SDK, schema, and tests can keep using ACP SDK types where protocol data is part of their public surface.
- Do not change daemon IPC method names, payload shapes, session persistence, or transcript rendering as part of this migration.
- Do not preserve daemon ACP modules or APIs just to reduce import churn. The repo is pre-alpha, so update call sites directly and delete redundant local modules.
- Do not add compatibility shims, adapter modules, or wrapper layers around `acp-client` unless the wrapper owns real Goddard behavior that `acp-client` intentionally does not own.
- Do not move host-owned behavior into `acp-client`. Permission policy, session history, prompt queueing, cancellation semantics, inbox integration, diagnostics, and transcript persistence remain Goddard daemon responsibilities.

## Migration Plan

### 1. Add The Package Dependency

- Add `acp-client` to the root catalog and `core/daemon/package.json`.
- Pin to the current published version first, then upgrade deliberately when `acp-client` publishes API changes.
- Run `bun install` so `bun.lock` records the package and its exact dependency tree.
- Keep `@agentclientprotocol/sdk` in the workspace catalog for packages that still import it directly. Daemon code can remove its direct dependency once `acp-client` also exposes every runtime ACP constant and helper it needs.

### 2. Replace Low-Level ACP Helpers Directly

- Update daemon ACP client call sites to import from `acp-client` directly:
  - `import type { acp } from "acp-client"` for protocol types that can be type-only;
  - `import { createAgentConnection, createAgentMessageStream, getAcpMessageResult, isAcpRequest, matchAcpRequest } from "acp-client"` for low-level transport helpers.
- Delete `core/daemon/src/session/acp.ts` in the same patch instead of turning it into a re-export module.
- Update affected tests to import helper functions from `acp-client` directly when they still need low-level helper assertions.
- Remove the daemon's direct `@agentclientprotocol/sdk` dependency after daemon ACP client code no longer imports it directly.
- Verify Bun-native subprocess stdin support before removing the local `FileSink` typing. If `acp-client` does not support the daemon's Bun `FileSink` path, fix `acp-client` first instead of preserving a Goddard-only bridge.

### 3. Preserve Session Manager Semantics

- Run focused daemon tests after the helper replacement:
  - `bun test --dots core/daemon/test/session-lifecycle.test.ts`;
  - `bun test --dots core/daemon/test/session-manager.test.ts`.
- Pay special attention to:
  - chunk logging through `AgentStreamHooks.onChunk`;
  - asynchronous subscriber error handling;
  - prompt response settlement via `getAcpMessageResult`;
  - permission request detection via `isAcpRequest`;
  - subscription close behavior during process exit and daemon shutdown.
- Fix behavior in `acp-client` if any low-level helper differs from Goddard's current semantics.

### 4. Migrate Raw Inspection To `createAcpClient`

- Update `core/daemon/src/session/inspect.ts` to use `createAcpClient` over a launched process instead of constructing `acp.ClientSideConnection` directly.
- Keep only the inspection data the debug CLI still needs:
  - `initialize`;
  - `session`;
  - `sessionList`;
  - `sessionUpdates`;
  - `close`.
- Use the `handler.sessionUpdate` callback to collect raw `session/update` notifications for the debug CLI.
- Keep `handler.requestPermission` returning a cancelled outcome, matching today's inspection behavior.

### 5. Decide How Far To Move Process Launch

- Compare Goddard's `spawnAgentProcess` and registry service behavior against `acp-client/node`:
  - daemon-specific environment variables such as daemon URL and token;
  - inline registry overrides from resolved Goddard config;
  - configured agent binary cache directory;
  - registry fallback generation;
  - process exit hooks and shutdown timing;
  - diagnostics currently emitted by the daemon.
- If `createNodeAcpClient` can accept all required launch inputs, replace Goddard's direct launch path for low-risk flows first, such as adapter inspection and launch preview.
- Keep live daemon sessions on Goddard-owned process handles until `acp-client` exposes every hook needed by `activateLiveSession`, especially raw chunk diagnostics, process exit diagnostics, and controlled shutdown.
- Remove `core/daemon/scripts/generate-acp-registry-fallback.ts` and local registry fallback code once `acp-client/node` owns registry cache and fallback behavior for daemon launches.

### 6. Remove Redundant Local Code

- After call sites use package-owned transport and any selected package-owned launch helpers, delete local code that has no remaining callers.
- Expected candidates:
  - `core/daemon/src/session/acp.ts`;
  - local registry cache/fallback helpers that duplicate `createAcpRegistryService`;
  - local binary archive extraction helpers that duplicate `acp-client/node`;
  - tests that only verify copied helper behavior already covered upstream.
- Keep tests that protect Goddard-specific behavior: daemon session lifecycle, reconnectability, transcript persistence, permission handling, prompt queueing, and IPC responses.

### 7. Verification

- Run focused daemon checks after each phase:
  - `bun run --cwd core/daemon typecheck`;
  - `bun run --cwd core/daemon test`.
- Run broader workspace checks before the final PR:
  - `bun run typecheck`;
  - `bun run test`.
- Exercise the ACP debug CLI manually:
  - `bun run acp adapter <adapter>`;
  - `bun run acp list <adapter>`;
  - `bun run acp stream --agent <adapter> "hello"`.
- Confirm no daemon IPC contract changed. If any daemon IPC method does change, update `app/src/daemon-ipc-test-handlers.ts` in the same patch.

## Rollout Order

1. Land dependency plus direct low-level helper imports and delete `core/daemon/src/session/acp.ts`.
2. Migrate raw inspection to `createAcpClient`.
3. Audit and, where practical, replace registry/process launch with `acp-client/node`.
4. Remove duplicated local registry, process, and archive code only after no daemon behavior depends on Goddard-specific hooks.

## Open Questions

- Should `acp-client` expose a Bun `FileSink`-compatible input type, or should Goddard normalize Bun process stdin before passing it to the package?
- Does `createNodeAcpClient` need raw chunk hooks and process exit hooks before it can replace live daemon session launch?
- Should Goddard continue using its configured cache directories, or should it adopt `acp-client`'s default cache locations under `XDG_CACHE_HOME` / `~/.cache/acp-client`?
- Should registry override precedence remain exactly as Goddard implements it today, or can it follow `acp-client`'s package-defined precedence?

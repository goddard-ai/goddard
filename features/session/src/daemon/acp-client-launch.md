# ACP Client Launch Integration

## Current Decision

Keep daemon session launch on Goddard's `spawnAgentProcess()` plus `createAcpClient()` path for now.

`acp-client/node` provides `createNodeAcpClient()`, but that helper launches the process, initializes the ACP client, and owns process shutdown as one unit. Goddard currently needs those concerns separated because the session manager owns daemon-specific launch policy, persistence, diagnostics, and launch leases.

## Why `createNodeAcpClient()` Is Not A Drop-In Replacement

Goddard daemon launch currently needs:

- `GODDARD_DAEMON_URL` and `GODDARD_SESSION_TOKEN` injected after distribution env resolution.
- `sessions.envPolicy` filtering applied across inherited host env, config-set env, agent distribution env, and per-session env.
- Process stderr mirrored to the daemon temp log directory for diagnostics.
- Launch-preview leases that keep a preinitialized process and ACP session alive before the durable daemon session is created.
- Managed install resolution through `DaemonAgentInstallService` when `agents.managed[agent].install === "beforeUse"`.
- Explicit process ownership so persisted session state, active-session maps, shutdown, and daemon reconciliation stay under the session manager boundary.

`createNodeAcpClient()` accepts `env`, `registry`, `registryService`, `binaryCacheDir`, and close policy, but it does not expose hooks for Goddard's environment assembly, stderr logging, launch-lease handoff, or managed-install process-spec resolution.

## Safe Delegation Boundary

The daemon should continue to use acp-client for:

- ACP protocol client/session lifecycle via `createAcpClient()`.
- Registry lookup through the daemon registry service backed by `createAcpRegistryService()`.
- Managed install/update/status/process-spec operations through `acp-client/node` managed install APIs.
- Low-level archive helpers only where Goddard still owns an explicit local adapter or unmanaged binary cache path.

The daemon should not move to `createNodeAcpClient()` until acp-client can provide process-launch hooks without taking over daemon-owned policy and persistence boundaries.

## Upstream Feature Request Draft

Title: Expose composable Node launch hooks for daemon-owned ACP session managers

Request:

`createNodeAcpClient()` is useful for standalone clients, but daemon session managers need to keep process launch, environment policy, logging, preinitialized session leases, and durable session persistence separate from ACP initialization.

Please consider exposing one of these smaller primitives:

- A Node launch helper that resolves the runnable process spec and accepts a host-owned environment builder before spawning.
- A process-launch callback in `createNodeAcpClient()` that receives the resolved spec and returns host-owned `{ stdin, stdout, close }` streams.
- A managed-install-aware process spec resolver that can be composed with host-owned process spawning and then passed to `createAcpClient()`.

Required behavior:

- Preserve existing registry and inline distribution resolution.
- Allow callers to provide `registryService`, inline `registry`, binary cache location, and managed install cache location.
- Let callers inject or filter environment variables after distribution env resolution and before spawn.
- Let callers observe or redirect stderr.
- Let callers keep the process handle separate from the initialized ACP client so launch-preview leases can transfer ownership into a later durable session.
- Keep shutdown policy explicit so hosts can coordinate ACP close, process termination, and durable session status updates.

This would let Goddard delegate more launch mechanics to acp-client without hiding daemon-owned session lifecycle policy.

# Daemon Cloud Sandbox Assessment

## Purpose

This report assesses what blocks running the Goddard daemon in a cloud sandbox such as Modal, Blaxel, or Daytona. It is ordered from lowest-hanging fruit to deeper architectural work.

The current daemon can plausibly run inside a single Linux sandbox today if the repo, daemon, agent process, and CLI client are all co-located in that same environment. The harder product is a remotely hosted daemon that the desktop app, SDK, or agents can safely address across a network. Most obstacles below are about that second case.

## Current Shape

The daemon is intentionally a local automation authority:

- `core/ipc/src/node/server.ts` exposes HTTP-over-TCP IPC, bound to `127.0.0.1` by default.
- `core/schema/src/daemon-url.ts` models daemon URLs as plain `http://` TCP URLs with explicit ports.
- `core/daemon/src/session/manager.ts` launches agent adapters as local subprocesses and talks ACP over stdio.
- `core/paths/src/node/index.ts` and `core/daemon/src/persistence/store.ts` place config, credentials, cache, and SQLite state under the active user's home directory.
- `core/daemon/src/worktrees/` assumes local git repositories and linked worktrees.
- `core/daemon/src/workforce/` stores workforce intent and ledger state inside the repository-local `.goddard` directory.
- `app/src/bun/daemon-runtime.ts` installs or reuses a user-scoped desktop daemon service and probes it on localhost.

That shape is coherent for a supervised desktop/local host. Cloud support needs clearer separation between control, execution, persistence, repository provisioning, and network exposure.

## Target Modes

### Mode A: Co-located Sandbox

Daemon, agent, repository checkout, and operator CLI run inside one sandbox. This is closest to today's architecture.

What works well:

- Localhost IPC remains acceptable.
- Local subprocess agents still work.
- Local git and worktrees still work.
- `~/.goddard` persistence can live on an attached volume.

Missing:

- Sandbox provisioning and bootstrap.
- Repository clone/fetch setup.
- Durable volume conventions.
- Non-desktop startup and shutdown commands.

### Mode B: Remote Daemon Endpoint

The daemon runs in a sandbox, while the desktop app, SDK, or other clients connect remotely.

This requires:

- Authenticated remote IPC.
- Transport-safe streams.
- Endpoint discovery.
- Explicit trust boundaries.
- Remote lifecycle management.

### Mode C: Remote Execution Host

The daemon remains a control authority, but agent execution happens in separate sandboxes or jobs.

This is the deepest change. It needs an execution-host abstraction instead of assuming local subprocesses and stdio.

## Lowest-Hanging Fruit First

### 1. Define the Supported Cloud Mode

What's missing:

- The repo does not distinguish "daemon runs inside a sandbox" from "daemon exposes a remote control endpoint" or "daemon schedules remote jobs."
- Current specs describe a local background runtime and another supervised local process, but not a cloud sandbox host.

What's needed:

- A short design note or ADR defining which mode comes first.
- A threat model for who can call daemon IPC.
- A lifecycle model for sandbox creation, reuse, suspend, resume, and teardown.

Special attention:

- Do not treat "bind to `0.0.0.0`" as cloud support. Without auth and endpoint ownership, that would expose session creation, filesystem-backed suggestions, PR actions, and workforce mutation to the network.

Why this is first:

- It prevents code work from drifting between three different products.

### 2. Add a Non-Desktop Launch Profile

What's missing:

- The app-managed daemon runtime is desktop-service oriented. It installs a user service or Windows Run registry entry from `app/src/bun/daemon-runtime.ts`.
- The daemon CLI can run directly, but there is no documented cloud profile that says where state, cache, agent binaries, logs, and repo checkouts live.

What's needed:

- A documented `cloud` or `sandbox` run profile.
- Environment variables for home, data profile, cache root, agent bin dir, base URL, and daemon URL.
- A simple entrypoint command that does not install desktop services.

Special attention:

- Keep this separate from the desktop app's service installer.
- Treat logs as stdout/stderr first, because cloud hosts already collect process logs.

Likely shape:

```sh
goddard-daemon run \
  --base-url "$GODDARD_BASE_URL" \
  --port "$PORT" \
  --agent-bin-dir "$GODDARD_AGENT_BIN_DIR"
```

### 3. Make Network Binding Explicit

What's missing:

- The IPC server accepts a `hostname` internally, but `startDaemonServer()` does not expose that option.
- `createDaemonUrl()` defaults to `127.0.0.1`.
- Daemon URL validation only accepts plain `http://` with no path, query, or hash.

What's needed:

- CLI and runtime config for IPC host binding.
- A separation between bind address and public daemon URL.
- Support for cloud-provided public endpoints, possibly with paths if a provider routes services under a URL prefix.

Special attention:

- Binding externally should require an authenticated mode.
- Some providers terminate TLS before the container, so the daemon may still listen on HTTP while clients use HTTPS through the provider URL.

### 4. Add Authenticated IPC

What's missing:

- Daemon IPC trusts local process boundaries.
- Session-scoped tokens exist for agent-to-daemon actions, but general IPC requests like `sessionCreate`, `sessionList`, `adapterList`, `workforceStart`, and `loopStart` do not have a network authentication layer.

What's needed:

- A bearer token or signed request mechanism for daemon IPC.
- Client support in `@goddard-ai/daemon-client` and `@goddard-ai/sdk`.
- Redaction and logging rules for daemon auth headers.
- Different auth scopes for operator clients and session agents.

Special attention:

- The current `GODDARD_SESSION_TOKEN` is not enough. It authenticates a daemon-managed session for specific agent callbacks; it should not become an all-powerful remote daemon credential.
- Long-lived cloud daemons need token rotation or short-lived endpoint credentials.

### 5. Make Remote Streams Robust

What's missing:

- IPC streams are long-lived NDJSON responses from `/stream`.
- The app bridge does not implement daemon stream subscriptions yet.
- There is no cloud-facing reconnect cursor, replay, heartbeat, or backpressure strategy.

What's needed:

- Heartbeats and reconnect behavior for remote stream consumers.
- A way to recover missed session and workforce events after disconnect.
- App and SDK support for stream subscription over the selected remote transport.

Special attention:

- Cloud routers often close idle HTTP connections.
- Workforce events can be reconstructed from the ledger, but session message streams are live process traffic and need a replay boundary if they are user-visible.

### 6. Create a Sandbox Host Adapter Layer

What's missing:

- There is no concept of a host provider that can start, inspect, stop, or resume a sandbox.
- The desktop app assumes it owns local daemon installation.
- The SDK assumes it is handed a daemon URL or resolves one from local env/config.

What's needed:

- A small provider interface for sandbox lifecycle:
  - create or reuse sandbox
  - expose daemon URL
  - attach or create durable volume
  - provision repository checkout
  - stop or suspend sandbox
  - fetch logs and status
- Provider implementations can then target Modal, Blaxel, Daytona, or local Docker without changing daemon internals.

Special attention:

- Keep this out of daemon core initially. The daemon should not need to know which provider launched it unless provider-specific metadata affects runtime behavior.

### 7. Make Repository Provisioning First-Class

What's missing:

- Most daemon APIs accept a `cwd` or `rootDir` and assume the repository already exists locally.
- PR feedback looks up a previous local checkout path from daemon persistence.
- Workforce reads `.goddard/workforce.json` and `.goddard/ledger.jsonl` from the repo checkout.

What's needed:

- A repository provisioning contract:
  - GitHub owner/repo
  - target ref or PR number
  - checkout path inside sandbox
  - credentials strategy
  - clone depth/fetch policy
  - reuse policy for existing checkouts
- A way to map local desktop project identity to remote sandbox paths.

Special attention:

- Absolute paths persisted by the daemon are only meaningful inside the sandbox that created them.
- PR feedback cannot rely on a user's local checkout path when the work is happening remotely.

### 8. Decide What State Must Survive Sandbox Restart

What's missing:

- Daemon state is local SQLite under `~/.goddard`.
- Workforce intent is repo-local append-only JSONL.
- Loop runtimes and active sessions are mostly in-memory.

What's needed:

- A persistence policy by data class:
  - auth tokens
  - daemon sessions and turn history
  - active stream/reconnect state
  - worktree metadata
  - workforce ledger
  - runtime locks and active ownership
  - installed adapter binaries and provider packages
- A durable volume layout or external store decision.

Special attention:

- Attached volumes solve many single-sandbox restart problems.
- They do not solve multiple sandboxes touching the same repo, same SQLite file, or same workforce ledger concurrently.

### 9. Abstract Agent Process Execution

What's missing:

- `SessionManager` directly resolves an adapter, starts `Bun.spawn`, wires stdin/stdout, and uses local process-tree kill behavior.
- ACP transport is coupled to child process stdio.

What's needed:

- An execution host abstraction that can launch an ACP adapter through:
  - local subprocess
  - sandbox job
  - sidecar process
  - remote session transport
- A process handle contract for stdin/stdout or equivalent message transport, exit state, cancellation, and cleanup.

Special attention:

- This is where Mode C starts. Do not do this first unless remote agent execution is the explicit target.
- Keep local subprocess execution as one implementation, not a fallback compatibility branch.

### 10. Revisit Worktree and Git Isolation

What's missing:

- The default worktree plugin creates linked git worktrees from a local repo.
- Fresh worktree preparation copies untracked artifacts and may run a local package-manager install.
- Worktree sync uses local git internals, local filesystem watchers, locks, and temporary directories.

What's needed:

- A cloud-safe workspace strategy:
  - shared checkout per repo
  - linked worktrees on an attached volume
  - separate clone per session
  - provider-native workspace snapshots
- Explicit cleanup and disk quota behavior.

Special attention:

- Linked worktrees are efficient but bind all sessions to one git common directory.
- Separate clones are simpler for isolation but cost more network, disk, and bootstrap time.
- Current workforce guidance expects shared working tree behavior. That assumption needs a deliberate cloud answer.

### 11. Add Runtime Ownership and Leases

What's missing:

- Active sessions, loop runtimes, workforce runtimes, running agents, and PR coalescing are process-local memory.
- The daemon assumes one process is the lifecycle authority for a given local runtime.

What's needed:

- Runtime ownership records with leases or fencing if multiple cloud daemons can exist for the same user/repo.
- Startup reconciliation that can tell whether an active runtime is still owned by this process, another process, or no live process.
- Idempotent start/reuse behavior across sandbox restarts.

Special attention:

- This is not optional if provider autoscaling or duplicate starts are possible.
- Local SQLite plus filesystem state is insufficient for distributed ownership unless all contenders share one reliable lock primitive.

### 12. Tighten Cloud Security Boundaries

What's missing:

- Remote filesystem suggestions can expose sandbox paths.
- Agent env currently inherits `process.env` before adding daemon-specific values.
- Dynamic provider-package install can run package managers in daemon-managed cache directories.
- Repo-local config can influence worktree bootstrap behavior.

What's needed:

- Clear allowlists for environment variables passed to agents.
- Secret injection rules.
- Network exposure rules.
- Package install policy for cloud daemons.
- Filesystem disclosure policy for remote clients.

Special attention:

- Cloud sandboxes make it easier to isolate risky work, but they also create new exfiltration paths through logs, remote IPC, and mounted volumes.

### 13. Add Operational Controls

What's missing:

- There is limited first-class cloud observability: no sandbox id, provider id, volume id, billing/cost metadata, or remote log attachment in daemon status.
- Loop and workforce runtimes expose domain status but not host-level resource state.

What's needed:

- Host metadata in daemon health/status.
- Runtime status that distinguishes daemon up, repo provisioned, agent active, waiting, failed, and cleanup pending.
- Log and diagnostic surfaces suitable for remote debugging.
- Quotas for disk, time, loop cycles, concurrent sessions, and package install behavior.

Special attention:

- Without quotas, unattended loops and workforce sessions can become open-ended cloud spend.

## Recommended Phasing

### Phase 1: Co-located Cloud Sandbox

Goal: prove the daemon can run headlessly in a provider sandbox with a repo checkout.

Work:

- Document Mode A as the first supported target.
- Add a non-desktop cloud launch guide.
- Parameterize data/cache roots if current `HOME` handling is not enough.
- Provision a repo checkout manually or through a small wrapper script.
- Keep IPC private to the sandbox or provider session.

Expected outcome:

- Useful for experiments and internal dogfooding.
- Does not yet support the desktop app connecting safely from outside the sandbox.

### Phase 2: Remote Daemon Control

Goal: let approved clients control a cloud-hosted daemon.

Work:

- Add authenticated IPC.
- Separate bind address from public daemon URL.
- Support cloud endpoint URLs.
- Harden streams with reconnect behavior.
- Add SDK/app connection configuration for remote daemon URLs.

Expected outcome:

- Desktop or CLI can talk to a cloud daemon without relying on local process boundaries.

### Phase 3: Provider-Managed Sandboxes

Goal: make cloud runtime lifecycle a product feature.

Work:

- Add sandbox host adapter interfaces.
- Implement one provider first.
- Add repository provisioning.
- Define durable volume layout and cleanup.
- Expose host status and logs.

Expected outcome:

- Users can start or reuse a remote daemon for a repository from an approved control surface.

### Phase 4: Distributed Execution

Goal: support agents running somewhere other than the daemon process host.

Work:

- Extract execution-host abstraction from `SessionManager`.
- Add remote ACP transport or sidecar job transport.
- Add distributed leases and ownership if multiple workers can process the same repo.
- Revisit worktree and git attribution rules for non-local execution.

Expected outcome:

- Daemon becomes a control authority that can schedule execution across one or more sandboxes.

## Important Open Questions

- Should cloud support start as a single-user sandbox, or as a multi-tenant hosted service?
- Is the daemon endpoint ever public, or does the product rely on provider tunnels and short-lived access URLs?
- Does a cloud daemon own a repo checkout per user, per repo, per PR, or per session?
- Where should auth tokens live: provider secrets, daemon SQLite on a volume, backend-issued short-lived tokens, or a mix?
- Should workforce continue to use repo-local `.goddard/ledger.jsonl` as the source of durable intent when running remotely?
- Do cloud sessions need sync back to a local desktop checkout, or is the remote checkout the source of truth until PR creation?
- Are package installs allowed during session startup, and how are cache poisoning and disk growth controlled?

## Risk Summary

The easiest path is not a large daemon rewrite. It is to run the existing daemon inside a single provisioned sandbox and make its launch/profile assumptions explicit.

The risky path is exposing today's daemon IPC over the network. That crosses a security boundary the current architecture intentionally avoids. Authenticated IPC, endpoint ownership, path disclosure rules, and stream recovery should come before remote desktop or SDK control.

The deepest path is remote agent execution independent of the daemon process. That requires extracting local subprocess assumptions from session management and making execution, cancellation, ACP transport, and workspace ownership provider-aware.

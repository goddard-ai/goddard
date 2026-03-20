# Goddard Workforce

Goddard Workforce is Goddard's repository-scoped multi-agent orchestration system.

It gives one repository:

- a root agent for repo-wide coordination
- domain agents for owned packages or paths
- a daemon-owned runtime for routing, recovery, and visibility

## Why It Exists

Workforce is how Goddard scales beyond a single coding session without losing control.

Work is durable, scoped, and auditable:

- `.goddard/workforce.json` defines the workforce
- `.goddard/ledger.jsonl` records the append-only history
- the daemon owns runtime lifecycle and queue state

## How It Works

The `goddard-workforce` CLI is the operator surface.

Typical flow:

1. Initialize a repository workforce.
2. Start the daemon-owned runtime for that repository.
3. Queue work for the root agent or a domain agent.
4. Let agents respond, suspend, or delegate through the injected `workforce` executable.
5. Inspect status through the daemon-backed CLI or SDK.

## Developer Model

Workforce is daemon-owned, not client-owned.

- `@goddard-ai/schema` defines workforce config, ledger events, and IPC contracts.
- `@goddard-ai/daemon` owns runtime, replay, queues, and per-request sessions.
- `@goddard-ai/sdk` exposes daemon-backed workforce helpers.
- `@goddard-ai/workforce` is the operator CLI.

Each handled request runs in a fresh daemon session with workforce metadata and routed commands.

## Status

Workforce is currently pre-alpha and headless. The daemon, SDK, schema, and CLI surfaces exist today. A dedicated app UI does not.

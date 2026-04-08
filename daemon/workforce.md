# Daemon-Owned Workforce Orchestration

## Goal
Enable repository-scoped multi-agent delegation where the daemon owns workforce lifecycle, recovery, and coordination rather than leaving those responsibilities to individual clients.

## Hypothesis
We believe that making the daemon the single authority for workforce runtime state will produce more reliable recovery, clearer operator visibility, and safer collaboration between agents and control surfaces.

## Actors
- Operator — starts, stops, inspects, and mutates workforce state through approved clients.
- Daemon Runtime — owns repository-scoped workforce lifecycle, validation, queue projection, and recovery.
- Root Agent — holds repository-wide coordination responsibility.
- Domain Agent — owns a constrained domain and handles delegated requests within that domain.
- SDK and Operational CLI Clients — thin control and observation surfaces for the daemon-owned runtime.

## State Model

`Stopped -> Recovering -> Idle -> Handling -> (Idle | Suspended | Failed) -> ShuttingDown -> Stopped`

## Core Behavior
1. An operator explicitly starts workforce orchestration for a repository workspace.
2. The daemon reconstructs workforce state from durable repository-local intent before admitting new work.
3. New work is recorded against the repository workforce and projected into the current queue state.
4. Each agent receives eligible requests sequentially within its owned domain.
5. Each handled request runs in a fresh agent session with ownership rules, recent workforce context, and daemon-enforced git controls.
6. Workforce domain agents intentionally share one repository working tree even when they have separate ownership boundaries.
7. Agents may respond, suspend, or delegate additional work through daemon-backed workforce controls.
8. A response is a validation gate rather than a blind completion signal: the daemon validates attributable git state and only then advances the queue.
9. Operators and clients inspect current workforce status through the daemon rather than by managing parallel watcher state.

## Hard Constraints
- The daemon is the sole lifecycle authority for workforce runtimes.
- Only one active workforce runtime may exist for a given repository workspace.
- Workforce startup is explicit and should safely reuse an already-running runtime for the same repository instead of creating duplicates.
- Durable workforce history must be sufficient to rebuild queue and request state after daemon restart.
- Requests for the same agent must never be handled concurrently.
- Workforce sessions may share one working tree, but daemon-enforced git controls must preserve per-request attribution strongly enough for validation and audit.
- Agent commits must happen through daemon-enforced workforce git controls rather than through an untracked raw git environment.
- Before queue advancement, the daemon must validate the responding request's attributable git state and commits against that agent's owned paths.
- A request must not complete while the responding agent still has dirty tracked changes inside its owned paths.
- If attributable git changes for a request touch paths outside the responding agent's owned paths, the daemon must suspend that request and surface the violation for human review.
- Workforce orchestration and PR feedback handling remain separate daemon runtime domains even when hosted by the same daemon process.

## Failure Handling Expectations
- Daemon restart should recover workforce state without losing operator-visible progress.
- Individual agent-session failure must not corrupt the broader workforce queue.
- Suspended work must remain blocked until an explicit operator or root-agent action resolves it.
- Ownership validation failures should suspend the violating request instead of allowing silent completion.
- Shutdown should stop new handling cleanly and preserve enough durable intent for later restart.

## Non-Goals
- Allowing SDK clients to own independent workforce runtime state
- Reopening Goddard as a broad interactive terminal-first product
- Defining data formats, file layouts, or command syntax in this spec

## Decision Memory
Workforce orchestration moved away from client-owned watcher and long-lived session concepts toward a daemon-owned runtime with fresh per-request sessions so recovery, auditability, and shared control surfaces stay aligned.

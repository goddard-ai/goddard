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
5. Each handled request runs in a fresh agent session with ownership rules and recent workforce context.
6. Agents may respond, suspend, or delegate additional work through daemon-backed workforce controls.
7. Operators and clients inspect current workforce status through the daemon rather than by managing parallel watcher state.

## Hard Constraints
- The daemon is the sole lifecycle authority for workforce runtimes.
- Only one active workforce runtime may exist for a given repository workspace.
- Workforce startup is explicit and should safely reuse an already-running runtime for the same repository instead of creating duplicates.
- Durable workforce history must be sufficient to rebuild queue and request state after daemon restart.
- Requests for the same agent must never be handled concurrently.
- Agent-issued changes must be validated and committed by the workforce runtime before queue advancement.
- Workforce orchestration and PR-feedback handling remain separate daemon runtime domains even when hosted by the same daemon process.
- This slice remains headless; it does not require a dedicated app UI.

## Failure Handling Expectations
- Daemon restart should recover workforce state without losing operator-visible progress.
- Individual agent-session failure must not corrupt the broader workforce queue.
- Suspended work must remain blocked until an explicit operator or root-agent action resolves it.
- Shutdown should stop new handling cleanly and preserve enough durable intent for later restart.

## Non-Goals
- Allowing SDK clients to own independent workforce runtime state
- Reopening Goddard as a broad interactive terminal-first product
- Defining data formats, file layouts, or command syntax in this spec

## Decision Memory
Workforce orchestration moved away from client-owned watcher and long-lived session concepts toward a daemon-owned runtime with fresh per-request sessions so recovery, auditability, and shared control surfaces stay aligned.

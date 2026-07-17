# Repository Tasks

Tasks provide durable, daemon-owned planning state for work associated with a repository.

## Core Model

- Every task belongs to one repository root.
- A task records a title, optional description, priority, lifecycle status, optional blocking reason, optional claiming session, and timestamps.
- Task status is limited to `todo`, `active`, `blocked`, `done`, and `cancelled`.
- Current task state is canonical. Task activity is an immutable audit history and is not a replay or synchronization contract.
- Tasks remain distinct from sessions, workforce requests, inbox items, pull requests, and review sessions. Those entities may be linked to a task without transferring ownership of their state or lifecycle.

## Capabilities

- Approved daemon clients can create tasks, inspect one task, and list tasks for a repository.
- Clients can edit task content and priority and can move tasks through the supported lifecycle states.
- Clients can add immutable notes to a task.
- Clients can add and remove generic links to Goddard entities, repository resources, or external resources.
- Clients can inspect a task's links and activity history.
- Task lists and activity history use stable, deterministic ordering.
- The SDK exposes the same task behavior as the daemon control surface.

## Claims

- A task may be claimed by one daemon-managed session at a time.
- Claim and release operations are atomic.
- Claiming a task already held by another session fails explicitly and does not replace the existing claim.
- Claims do not expire automatically. Releasing an abandoned claim requires an explicit client action.

## Activity

Every accepted mutation records an activity entry as part of the same durable action. Activity identifies what changed, when it changed, and the responsible actor when that identity is available.

A failed or rejected mutation changes neither current task state nor activity history.

## Boundaries

- The daemon is the sole authority for mutable task state. Clients must not maintain or mutate a parallel task store.
- Tasks coordinate planned work; they do not supervise agent execution or replace workforce requests.
- The task lifecycle does not mirror pull request or review lifecycle state.
- Task links do not cache or redefine state owned by another feature or external provider.
- This capability does not provide task dependencies, parent-child hierarchy, custom workflows, attachments, rich text, expiring leases, remote synchronization, or task-specific backup.
- Activity history is not required to reconstruct task state and does not provide branching, merging, checksums, snapshots, or compaction.
- This spec does not define command syntax, payload shapes, database structure, or user-interface design.

# Task Feature

`@goddard-ai/task` owns durable repository task planning across daemon IPC and the SDK.

## Domain

- A task belongs to one normalized repository root.
- Current task state is canonical and uses the `todo`, `active`, `blocked`, `done`, and `cancelled` lifecycle.
- Each accepted mutation appends immutable activity in the same transaction.
- A task may be claimed by one daemon-managed session at a time. Claims remain until an explicit release.
- Notes are immutable activity. Links are task-owned references and do not copy state from sessions, workforce requests, pull requests, files, or URLs.

## Boundaries

The feature owns task schemas, persistence, mutation rules, IPC, events, and SDK namespace construction. Daemon process and database lifecycle remain core substrate. Sessions continue to own execution, workforce requests own delegated runtime work, and inbox items own human attention state.

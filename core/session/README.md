# @goddard-ai/session

Session client bindings for daemon-hosted ACP sessions.

## What this package provides

- `runAgent()` for creating/connecting sessions through the daemon IPC server
- `AgentSession` helpers for prompt/cancel/history/shutdown operations
- ACP transport wiring over daemon IPC requests + ndJSON streams

The daemon owns all agent subprocesses and session lifecycle state.
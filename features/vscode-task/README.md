# `@goddard-ai/vscode-task`

Daemon-backed workspace task support for the custom-task subset implemented by
`vscode-tasks-engine`.

## Boundary

This package owns:

- `.vscode/tasks.json` loading, inspection, and structured errors
- resolved task previews
- explicit task-graph execution and cancellation
- connection-scoped lifecycle and PTY output streams
- the `vscodeTask` SDK namespace

The task engine owns parsing, supported-task analysis, variable and override
resolution, dependency ordering, fail-fast behavior, and cancellation
propagation. `@goddard-ai/terminal` owns PTY process realization and daemon
shutdown cleanup.

This package does not own app UI, persistence, automatic task execution,
Goddard Actions, VS Code extension task providers, problem matchers, background
task readiness, inputs, file-context variables, or multi-root workspaces.

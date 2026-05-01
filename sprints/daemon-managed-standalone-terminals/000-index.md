# Sprint: daemon-managed-standalone-terminals

## Objective

Deliver daemon-managed terminal instances that are owned by the desktop Bun host, transported over a daemon websocket, scoped by connection-local instance ids, disposed when that host disconnects, and surfaced in the app as a real standalone terminal tab with shared terminal state instead of the current debug-only local fixture shell.

## Base Branch Assumption

`codex/add-custom-terminal-input`

## Queue

1. `010-define-terminal-contract-and-lifecycle`
2. `020-validate-daemon-pty-runtime`
3. `030-add-daemon-terminal-websocket-and-sdk-surface`
4. `040-add-bun-host-terminal-bridge`
5. `050-add-standalone-terminal-tab-state-and-view`

## Review Flow

- Review `010-define-terminal-contract-and-lifecycle` before later transport or UI code is treated as stable.
- Treat `020-validate-daemon-pty-runtime` as a stop/go checkpoint.
- Review `030-add-daemon-terminal-websocket-and-sdk-surface` as the public daemon control-surface checkpoint.
- Review `040-add-bun-host-terminal-bridge` before final app-surface work.
- Review `050-add-standalone-terminal-tab-state-and-view` as the first full product slice.

## Sprint Constraints

- The Bun host owns the daemon terminal connection; the webview stays behind Electrobun RPC and messages.
- Terminal instances are disposed on Bun-host disconnect in this sprint.
- The first standalone terminal is a general utility tab, not a session-attached terminal artifact surface.
- The terminal surface must stay a bounded utility, not a terminal-first primary workflow.
- Shared daemon-backed app capabilities must also be represented in `core/sdk/`.


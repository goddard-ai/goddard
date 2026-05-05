# Task: 020-validate-daemon-pty-runtime

## Status

finished-unreviewed

## Objective

Prove the daemon can safely own PTYs with `bun-pty` before daemon terminal streams and app work depend on it.

## Scope

- Move or add the PTY dependency to the daemon package.
- Add a daemon-owned terminal runtime or manager skeleton.
- Verify PTY spawn, write, resize, and close in the daemon's actual runtime and packaging path.

## Dependencies

- `010-define-terminal-contract-and-lifecycle` accepted or at least stable enough for daemon PTY validation.

## Acceptance Criteria

- The daemon can spawn and control a PTY using the chosen library.
- Terminal runtime cleanup is deterministic when a runtime is explicitly closed.
- Validation covers the daemon's real execution mode closely enough to de-risk standalone shipping, not just an isolated local script.
- Any packaging or native-module handling required by the daemon build is identified and implemented here.

## Review Checkpoint

The human is reviewing whether daemon PTY ownership is technically viable and worth building on.

## Review Report

### Plain-English Summary

This task proves the daemon can own terminal PTYs directly with `bun-pty`, including spawn, input, resize, restart, close, and deterministic cleanup. It also verifies the packaged standalone daemon can load and exercise the native PTY dependency.

### How To Verify Without Reading Code

Review the reported daemon terminal checks and standalone build test results. Acceptance means the daemon can create and control a PTY in its real runtime path, and the sprint can build terminal request handlers and stream delivery on top of this daemon-owned runtime.

### Agent Verification

- `bun test core/daemon/test/terminal-runtime.test.ts`
- `bun run --cwd core/daemon src/main.ts terminal-check --json`
- `bun run --cwd core/daemon typecheck`
- `bun run --cwd core/daemon lint` passed with 0 errors and one existing warning in `test/daemon.test.ts`.
- `bun test core/daemon/test/standalone-build.test.ts`
- `bun run --cwd core/daemon test`

### Approval Questions

- Is daemon-side PTY ownership with `bun-pty` viable enough to build the HTTP request and daemon stream surface on top of it?
- Is the standalone packaging validation sufficient to proceed without changing the PTY library or packaging strategy?

### Known Limits

- The task validates daemon PTY ownership, not the final HTTP request endpoints, SDK methods, or app terminal tab UI.
- One task ahead is not safe until this is approved because terminal transport and app bridge work depend on PTY runtime viability.

## Work-Ahead Safety

One task ahead is not safe. If daemon PTY viability or packaging behavior changes, terminal stream and app work would likely need rework.

## Implementation Notes

- Added `bun-pty` as a daemon package dependency.
- Added a daemon terminal manager that owns one connection-local set of PTYs, supports create/input/resize/restart/close requests, and closes all runtimes deterministically.
- Added a `goddard-daemon terminal-check --json` diagnostic path that exercises daemon PTY spawn, write, resize, and close.
- Extended the standalone build test so the compiled daemon binary runs `terminal-check`, verifying native PTY packaging in the actual daemon artifact.

## Verification Evidence

- `bun test core/daemon/test/terminal-runtime.test.ts`
- `bun run --cwd core/daemon src/main.ts terminal-check --json`
- `bun run --cwd core/daemon typecheck`
- `bun run --cwd core/daemon lint` passed with 0 errors and one existing warning in `test/daemon.test.ts`.
- `bun test core/daemon/test/standalone-build.test.ts`
- `bun run --cwd core/daemon test`

## Feedback Notes

- Rebased after the terminal contract changed from websockets to HTTP requests plus daemon streams.
- Updated the daemon terminal manager to scope events and requests with a terminal `connectionId`.

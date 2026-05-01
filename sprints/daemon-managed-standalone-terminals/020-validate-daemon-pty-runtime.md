# Task: 020-validate-daemon-pty-runtime

## Status

planned

## Objective

Prove the daemon can safely own PTYs with `bun-pty` before websocket and app work depend on it.

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

## Work-Ahead Safety

One task ahead is not safe. If daemon PTY viability or packaging behavior changes, websocket and app work would likely need rework.


---
id: cli-interactive-commands
status: ACTIVE
links:
  - type: Extends
    target: spec/architecture.md
  - type: Depends-On
    target: spec/data-flows.md
  - type: Relates-To
    target: spec/daemon/pr-feedback-one-shot.md
---

# CLI Specification — Interactive Mode

This node defines root `goddard` commands for manual terminal use. For autonomous loop commands, see [`cli/loop.md`](./loop.md).

## Actor

**Interactive Developer** — authenticates via GitHub Device Flow and runs commands directly.

## Commands

### `goddard login`

Options:
- `--username <github-user>`

Behavior:
- Starts Device Flow via backend.
- Displays `user_code` and `verification_uri` for browser authorization.
- Polls until authorized, then persists the session token via `TokenStorage` (`~/.goddard/config.json`).
- Under normal network conditions, completes within 60 seconds.

### `goddard whoami`

Behavior:
- Reads local token and resolves associated GitHub identity from backend.
- Returns a clear auth error when no token exists.

### `goddard pr create`

Options:
- `--repo <owner/repo>` (auto-inferred from `.git/config` when possible)
- `--title <string>`
- `--head <branch>`
- `--base <branch>`

Behavior:
- Delegates creation intent through `sdk.pr.create()`.
- Backend creates PR as `goddard[bot]` with developer attribution text.
- Prints created PR URL.

### `goddard spec`

Behavior:
- Spawns local `pi` with the specification-guardian prompt.
- Uses inherited stdio for interactive operation.

### `goddard propose [...prompt]`

Behavior:
- Spawns local `pi` with proposal-review prompt.
- Forwards positional arguments as prompt content.

### `goddard agents init`

Behavior:
- Updates local `.pi/agent/AGENTS.md` guidance via SDK helper.
- Prints updated path.

## Failure and Degradation Expectations

- Missing auth state must produce an actionable re-login error.
- If repo inference fails, command requires explicit `--repo` and explains why.
- Network/API failures must surface deterministic, human-readable errors without stack-trace noise by default.

## Exit Behavior

| Situation | Exit code |
|-----------|-----------|
| Operational or config error | `1` |
| Successful command completion | `0` |

Errors should be presented in human-readable form suitable for direct terminal use.

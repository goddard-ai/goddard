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

This document covers the `goddard` root commands used by a human developer in an interactive terminal session. For autonomous loop commands, see [`cli/loop.md`](./loop.md).

---

## Actors

**Interactive Developer** — authenticated via GitHub Device Flow; executes commands manually from a terminal.

---

## Command: `goddard login`

Initiates GitHub Device Flow authentication.

### Options
- `--username <github-user>` — GitHub username to authenticate as.

### Behavior
- Requests a device code from the backend Worker.
- Displays the `user_code` and `verification_uri` for the developer to authorize in a browser.
- Polls the backend until the session is authorized.
- Persists the session token to `~/.goddard/config.json` via `TokenStorage`.
- Completes in under 60 seconds under normal network conditions.
- Prints a success confirmation with the authenticated GitHub username.

---

## Command: `goddard whoami`

Displays the currently authenticated GitHub identity.

### Behavior
- Reads the session token from `TokenStorage`.
- Queries the backend to resolve and display the associated GitHub username.
- Fails with a clear error if no session token is present.

---

## Command: `goddard pr create`

Creates a pull request attributed to the authenticated developer via `goddard[bot]`.

### Options
- `--repo <owner/repo>` — target repository (auto-inferred from `.git/config` if omitted).
- `--title <string>` — PR title.
- `--head <branch>` — source branch.
- `--base <branch>` — target branch.

### Behavior
- Resolves `owner/repo` from `.git/config` when `--repo` is omitted.
- Sends a PR creation request to the backend via `sdk.pr.create()`.
- The backend creates the PR as `goddard[bot]` and appends "Authored by @username" to the description.
- Prints the created PR URL on success.

---

## Command: `goddard spec`

Launches a focused `pi` session with the specification-guardian system prompt.

### Behavior
- Spawns local `pi` process with the spec-focused system prompt.
- Inherits terminal stdio so the session is fully interactive.

---

## Command: `goddard propose [...prompt]`

Launches a focused `pi` session with the proposal-review system prompt.

### Behavior
- Spawns local `pi` process with the proposal-focused system prompt.
- Passes remaining positional arguments through as prompt content.

---

## Command: `goddard agents init`

Appends Goddard spec instructions to local agent configuration.

### Behavior
- Delegates to SDK helper to update `.pi/agent/AGENTS.md` guidance in the current project.
- Prints the updated path.

---

## Exit Behavior

| Situation | Exit code |
|-----------|-----------|
| Operational or config error | `1` |
| Successful command completion | `0` |

All errors are written to stdout as human-readable messages.

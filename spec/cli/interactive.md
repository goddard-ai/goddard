---
id: cli-interactive-commands
status: ACTIVE
links:
  - type: Extends
    target: spec/architecture.md
  - type: Depends-On
    target: spec/data-flows.md
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

## Command: `goddard actions trigger`

Triggers a GitHub Actions workflow run.

### Options
- `--repo <owner/repo>` — target repository.
- `--workflow <name>` — workflow filename or ID.
- `--ref <ref>` — Git ref to run the workflow on (branch, tag, or SHA).

### Behavior
- Delegates to the backend, which calls the GitHub Actions API via the installed GitHub App.
- Prints a confirmation with the triggered workflow run URL.

---

## Command: `goddard stream`

Opens a real-time WebSocket subscription to repository events.

### Options
- `--repo <owner/repo>` — repository to subscribe to (auto-inferred from `.git/config` if omitted).

### Behavior
- Opens a WebSocket connection to the backend Durable Object for the specified repository.
- Begins printing events (comments, reviews) to the terminal within 2 seconds of a GitHub event firing.
- Formats each event with timestamp, actor, and event type for readability.
- Runs indefinitely until the process is interrupted (Ctrl-C).

---

## Exit Behavior

| Situation | Exit code |
|-----------|-----------|
| Operational or config error | `1` |
| Successful command completion | `0` |
| Stream interrupted by user | `0` |

All errors are written to stdout as human-readable messages.

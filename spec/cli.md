# CLI Specification

## Overview

The `goddard` CLI provides a unified interface for real-time GitHub terminal interactions and autonomous agent orchestration (via `pi-loop`).

## Goddard Core Commands

### Command: `goddard login`

Authenticates the CLI using GitHub's Device Flow.

#### Behavior
- Connects to the Goddard backend.
- Prompts user to authorize via the browser.
- Saves session tokens securely via the SDK's `TokenStorage`.

### Command: `goddard pr create`

Delegates PR creation to the Goddard App.

#### Options
- `--repo`: Target repository (`owner/repo`). Inferred from local `.git/config` if absent.
- `--title`, `--head`, `--base`: Standard PR parameters.

#### Behavior
- Uses the `goddard[bot]` installation token to create a PR via the GitHub API.
- Appends `Authored via CLI by @username` to the PR body for attribution.

### Command: `goddard actions trigger`

Triggers a specified GitHub Action workflow.

#### Options
- `--repo`: Target repository.
- `--workflow`: Name or ID of the workflow.
- `--ref`: The git ref to run the workflow against (e.g., `main`).

### Command: `goddard stream`

Subscribes to real-time repository events.

#### Options
- `--repo`: Target repository.

#### Behavior
- Establishes a WebSocket connection to a Cloudflare Durable Object.
- Streams events (e.g., comments, reviews) natively into the terminal using formatting and colors.

---

## Agent Orchestration Commands (`pi-loop`)

### Command: `goddard loop init` (or `pi-loop init`)

Creates `pi-loop.config.ts` using a default template.

#### Options
- `--global` / `-g`: write to home directory instead of current directory.

#### Behavior
- Fails if target config file already exists.
- Prints created path and next step (`goddard loop run`).

### Command: `goddard loop run` (or `pi-loop run`)

Loads config (`local` then `global`) and starts endless loop execution.

#### Behavior
- Fails with guidance if no config is found.
- Loads TypeScript config via `jiti`.
- Requires default export (or module object fallback).
- Instantiates loop and calls `start()`.
- If loop ends due to `DONE`, prints completion logs and exits successfully.

### Command: `goddard loop generate-systemd` (or `pi-loop generate-systemd`)

Generates a `pi-loop.service` file under `./systemd` (or `~/systemd` with `--global`).

#### Options
- `--global` / `-g`: use global config and output path rooted in home directory.

#### Behavior
- Reads `systemd` tuning values from config where available.
- Uses configurable `User` and `WorkingDirectory` when provided.
- Emits `Environment=` lines for defined `systemd.environment` entries.
- Emits a basic service file with:
  - `ExecStart=goddard loop run`
  - `Restart=always`
  - configurable `RestartSec` and `Nice`

---

## Exit behavior

- Operational/configuration errors exit process with status code `1`.
- `DONE` completion exits with status code `0`.
- Successful commands print human-readable progress logs.

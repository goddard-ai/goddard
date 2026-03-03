---
id: cli-loop-commands
status: ACTIVE
links:
  - type: Depends-On
    target: spec/configuration.md
  - type: Depends-On
    target: spec/runtime-loop.md
  - type: Relates-To
    target: spec/cli/interactive.md
---

# CLI Specification — Autonomous Loop

This document covers the `goddard loop` subcommand group, which manages autonomous agent cycles. For interactive developer commands, see [`cli/interactive.md`](./interactive.md).

---

## Actors

**Operator** — configures and supervises a long-running autonomous agent loop against a codebase.

---

## Command: `goddard loop init`

Creates `goddard.config.ts` from a default template.

### Options
- `--global` / `-g` — write to home directory (`~/.goddard/config.ts`) instead of current directory.

### Behavior
- Fails with a clear error if the target config file already exists.
- Prints the created path and the suggested next step (`goddard loop run`).

---

## Command: `goddard loop run`

Loads config (local, then global) and starts loop execution.

### Behavior
- Fails with actionable guidance if no config file is found at either location.
- Loads the TypeScript config file via `jiti` (no pre-compile step required).
- Requires a default export; accepts a module-object fallback.
- Instantiates the loop and calls `start()`.
- Prints cycle logs when `metrics.enableLogging` is `true`.
- If the loop terminates due to `DONE`, prints completion logs and exits with code `0`.

---

## Command: `goddard loop generate-systemd`

Generates a `goddard.service` systemd unit file for production deployment.

### Options
- `--global` / `-g` — use global config; output path rooted in home directory.

### Output path
- Local: `./systemd/goddard.service`
- Global: `~/systemd/goddard.service`

### Behavior
- Reads `systemd` tuning values from config when available.
- Uses configurable `User` and `WorkingDirectory` when provided.
- Emits `Environment=` lines for each defined entry in `systemd.environment`.
- Emits a service file with:
  - `ExecStart=goddard loop run`
  - `Restart=always`
  - Configurable `RestartSec` and `Nice`

### Example output
```ini
[Unit]
Description=Goddard Autonomous Agent Loop
After=network.target

[Service]
Type=simple
User=deployer
WorkingDirectory=/opt/myproject
ExecStart=goddard loop run
Restart=always
RestartSec=5
Nice=10
Environment=MY_VAR=value

[Install]
WantedBy=multi-user.target
```

---

## Exit Behavior

| Situation | Exit code |
|-----------|-----------|
| Operational or config error | `1` |
| `DONE` signal from agent | `0` |
| Successful command completion | `0` |

All errors and progress events are written to stdout as human-readable log lines.

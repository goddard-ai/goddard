# CLI Specification

## Command: `pi-loop init`

Creates `pi-loop.config.ts` using a default template.

### Options
- `--global` / `-g`: write to home directory instead of current directory.

### Behavior
- Fails if target config file already exists.
- Prints created path and next step (`pi-loop run`).

## Command: `pi-loop run`

Loads config (`local` then `global`) and starts endless loop.

### Behavior
- Fails with guidance if no config is found.
- Loads TypeScript config via `jiti`.
- Requires default export (or module object fallback).
- Instantiates loop and calls `start()`.

## Command: `pi-loop generate-systemd`

Generates a `pi-loop.service` file under `./systemd` (or `~/systemd` with `--global`).

### Options
- `--global` / `-g`: use global config and output path rooted in home directory.

### Behavior
- Reads `systemd` tuning values from config where available.
- Emits a basic service file with:
  - `ExecStart=pi-loop run`
  - `Restart=always`
  - configurable `RestartSec` and `Nice`

## Exit behavior

- Operational/configuration errors exit process with status code `1`.
- Successful commands print human-readable progress logs.

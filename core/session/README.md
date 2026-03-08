# `@goddard-ai/session`

Session runtime with embedded, dynamically loaded drivers.

## Drivers

Current embedded drivers:

- `pi` — programmatic `AgentSession` via `@mariozechner/pi-coding-agent` (no subprocess)
- `gemini` — subprocess: `gemini --output-format stream-json`
- `codex` — subprocess: `codex exec --json`
- `pty` — PTY wrapper for arbitrary command argv

## CLI

This package exposes a `session` cmd-ts executable.

From this package directory:

```bash
./bin/session <pi|gemini|codex|pty> ...
```

### `pi`

```bash
./bin/session pi "explain this repository"
./bin/session pi --resume <session-id-or-path> "continue from where we left off"
```

### `gemini`

```bash
./bin/session gemini "what is 1 + 1? no tool calls"
./bin/session gemini --resume <session-id> "add 1 to that"
```

### `codex`

```bash
./bin/session codex "what is 1 + 1?"
./bin/session codex --resume <thread-id> "add 1 to that"
```

### `pty`

```bash
./bin/session pty bash
./bin/session pty python3
./bin/session pty gemini --output-format stream-json --prompt "hello"
```

## Dynamic loading model

Drivers are currently embedded in this package but loaded via dynamic imports through the driver registry.

This gives us a driver architecture now, while still keeping first-party drivers hard-coded during this phase.

## JSON-RPC server notes

Public packages:

- `@goddard-ai/session`
- `@goddard-ai/session-client`
- `@goddard-ai/session-protocol`

`startServer(...)` now supports:

- `session_initialize`
- `session_send_event { event }`
- `session_get_state`

Server notifications emitted to connected websocket clients:

- `session_event { sequence, event }`

Driver input events:

- `input.text { text }`
- `input.terminal { data }`
- `terminal.resize { cols, rows }`

Driver output events:

- `output.text { text }`
- `output.terminal { data }`
- `output.normalized { payload }`
- `session.exit { exitCode }`
- `session.error { message }`

The `output.normalized` payload is driver-agnostic (`schemaVersion: 1`) and intended for persistence across mixed driver sessions.

For terminal-native streams (for example PTY), the server emits `payload.kind: "terminal"` with the latest screen snapshot (`cols`, `rows`, `lines`, `cursor`) after terminal output events.

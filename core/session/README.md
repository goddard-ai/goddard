# `@goddard-ai/session`

Session runtime with embedded, dynamically loaded plugins.

## Plugins

Current embedded plugins:

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

Plugins are currently embedded in this package but loaded via dynamic imports through the plugin registry.

This gives us a plugin architecture now, while still keeping first-party plugins hard-coded during this phase.

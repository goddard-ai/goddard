# @goddard-ai/session

Session orchestration for ACP-compatible agents.

## What this package provides

- Session server bootstrap and lifecycle management
- ACP websocket bridge for client ↔ agent communication
- Session metadata persistence via `@goddard-ai/storage`
- CLI tools under `src/bin/`

## Client demo

A local-first demo client is available at:

- `core/session/src/bin/client-demo.ts`

By default, it uses a **custom local ACP example agent** (no remote auth/token usage).

### Build first

From repo root:

```bash
pnpm --filter @goddard-ai/session build
```

### Run demo (local/no auth)

```bash
node core/session/dist/bin/client-demo.mjs
```

### Run demo with real-world auth flow

```bash
node core/session/dist/bin/client-demo.mjs --enable-auth
```

### Useful flags

- `--agent <name|json>`: registry name (e.g. `claude-code`) or inline JSON `AgentDistribution`
- `--agent-file <path>`: path to JSON `AgentDistribution` file
- `--cwd <path>`: working directory passed to the session
- `--prompt <text>`: prompt to send after startup
- `--help`: print usage

Examples:

```bash
node core/session/dist/bin/client-demo.mjs --cwd "$PWD" --prompt "Summarize this project"
node core/session/dist/bin/client-demo.mjs --agent claude-code
node core/session/dist/bin/client-demo.mjs --agent '{"type":"binary","cmd":"node","args":["./my-local-agent.js"]}'
node core/session/dist/bin/client-demo.mjs --agent-file ./agent-distribution.json
```

# `@goddard-ai/sdk`

`@goddard-ai/sdk` is the stable integration surface for Goddard platform capabilities.

## Related Docs

- [SDK Glossary](./glossary.md)
- [Agent Loop Domain Concepts](./src/loop/run-agent-loop.md)

## Package Surfaces

| Import | Owns | Does not own |
| --- | --- | --- |
| `@goddard-ai/sdk` | Backend HTTP API access, auth, PR operations, unified stream subscription | Daemon session lifecycle, runtime loop helpers |
| `@goddard-ai/sdk/daemon` | Daemon-backed agent sessions (`runAgent`, `AgentSession`) | Low-level daemon URL parsing and IPC transport factories |
| `@goddard-ai/sdk/loop` | Daemon-backed loop lifecycle control | Backend HTTP API access, host-specific IPC wiring |
| `@goddard-ai/sdk/node` | Node composition helpers and env-driven conveniences | Cross-environment transport ownership |

## `daemon-client` vs `sdk/daemon`

Use `@goddard-ai/daemon-client` when you need to:

- Resolve daemon connection settings from environment variables.
- Parse or construct daemon URLs.
- Bind a host-specific IPC client implementation such as Node sockets or Tauri IPC.

Use `@goddard-ai/sdk/daemon` when you need to:

- Create or reconnect daemon-backed agent sessions.
- Send prompts through ACP.
- Read daemon-managed session history.
- Shut sessions down through the daemon contract.

Fresh daemon-backed sessions use isolated worktrees when the provided `cwd` lives inside a git repository. Set `worktree: { enabled: false }` to force a session to run directly in the original checkout instead. Non-repository directories keep using the original `cwd`.

`daemon-client` owns transport setup. `sdk/daemon` owns session semantics.

## Environment Behavior

`@goddard-ai/sdk/daemon` and `@goddard-ai/sdk/loop` accept explicit daemon connection options for non-Node hosts.

- Node convenience path: omit options and rely on `GODDARD_DAEMON_URL` when the host process provides it.
- App path: pass an explicit `daemonUrl` and injected `createClient` factory. Do not rely on Node defaults in the app.

## Configuration Layout

Node helpers resolve persisted config from JSON only:

- Global defaults: `~/.goddard/config.json`
- Local defaults: `<repo>/.goddard/config.json`
- Prompt-only actions: `.goddard/actions/<name>.md`
- Packaged actions: `.goddard/actions/<name>/prompt.md` + `.goddard/actions/<name>/config.json`
- Packaged loops: `.goddard/loops/<name>/prompt.js` + `.goddard/loops/<name>/config.json`

Persisted prompt frontmatter is not supported. Loop `nextPrompt` logic comes from `prompt.js`, not JSON.
Started loops are daemon-owned background runtimes, so the initiating SDK host process may exit after startup without stopping the loop.

## Examples

Backend-only SDK usage:

```ts
import { GoddardSdk } from "@goddard-ai/sdk"

const sdk = new GoddardSdk({
  backendUrl: "https://api.example.com",
})

const session = await sdk.auth.login({
  githubUsername: "alec",
  onPrompt(verificationUri, userCode) {
    console.log(`Open ${verificationUri} and enter ${userCode}`)
  },
})

const pr = await sdk.pr.create({
  owner: "acme",
  repo: "widgets",
  title: "Ship the SDK contract",
  head: "feat/sdk-contract",
  base: "main",
})
```

Daemon session client usage:

```ts
import { runAgent } from "@goddard-ai/sdk/daemon"

const session = await runAgent(
  {
    agent: "pi",
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  },
  undefined,
  {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
  },
)

await session.prompt("Review the current diff.")
const history = await session.getHistory()
await session.stop()
```

Loop daemon lifecycle usage:

```ts
import { startDaemonLoop } from "@goddard-ai/sdk/loop"

const loop = await startDaemonLoop(
  {
    rootDir: process.cwd(),
    loopName: "triage",
    promptModulePath: "/workspace/.goddard/loops/triage/prompt.js",
    session: {
      agent: "pi",
      cwd: process.cwd(),
      mcpServers: [],
      systemPrompt: "Follow repository conventions.",
    },
    rateLimits: {
      cycleDelay: "30s",
      maxOpsPerMinute: 4,
      maxCyclesBeforePause: 200,
    },
    retries: {
      maxAttempts: 3,
      initialDelayMs: 500,
      maxDelayMs: 5_000,
      backoffFactor: 2,
      jitterRatio: 0.2,
    },
  },
  {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
  },
)

console.log(loop.sessionId)
```

Node convenience usage:

```ts
import { FileTokenStorage, GoddardSdk } from "@goddard-ai/sdk/node"

const sdk = new GoddardSdk({
  backendUrl: "https://api.example.com",
  tokenStorage: new FileTokenStorage(),
})

await sdk.actions.run("review", {
  cwd: process.cwd(),
  systemPrompt: "Use the local review checklist.",
})

await sdk.loop.start("triage", {
  session: {
    cwd: process.cwd(),
  },
})

const loops = await sdk.loop.list()
await sdk.loop.stop(process.cwd(), "triage")
```

App/Tauri daemon binding:

```ts
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { runAgent } from "@goddard-ai/sdk/daemon"

await runAgent(
  {
    agent: "pi",
    cwd: "/workspace",
    mcpServers: [],
  },
  undefined,
  {
    daemonUrl,
    createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
  },
)
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).

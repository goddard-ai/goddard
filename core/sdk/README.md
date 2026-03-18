# `@goddard-ai/sdk`

`@goddard-ai/sdk` is the stable integration surface for Goddard platform capabilities.

## Package Surfaces

| Import | Owns | Does not own |
| --- | --- | --- |
| `@goddard-ai/sdk` | Backend HTTP API access, auth, PR operations, repo stream subscription | Daemon session lifecycle, runtime loop helpers |
| `@goddard-ai/sdk/daemon` | Daemon-backed agent sessions (`runAgent`, `AgentSession`) | Low-level daemon URL parsing and IPC transport factories |
| `@goddard-ai/sdk/loop` | Loop/runtime orchestration on top of daemon sessions | Backend HTTP API access, host-specific IPC wiring |
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

`daemon-client` owns transport setup. `sdk/daemon` owns session semantics.

## Environment Behavior

`@goddard-ai/sdk/daemon` and `@goddard-ai/sdk/loop` accept explicit `RunAgentOptions`/daemon options for non-Node hosts.

- Node convenience path: omit options and rely on `GODDARD_DAEMON_URL` when the host process provides it.
- App path: pass an explicit `daemonUrl` and injected `createClient` factory. Do not rely on Node defaults in the app.

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

Loop usage:

```ts
import { runAgentLoop } from "@goddard-ai/sdk/loop"

await runAgentLoop(
  {
    nextPrompt: () => "Continue the current task.",
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
      retryableErrors: (error) => error instanceof Error,
    },
  },
  undefined,
  {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
  },
)
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

This project is licensed under the [MIT License](./LICENSE).

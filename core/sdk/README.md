# `@goddard-ai/sdk`

`@goddard-ai/sdk` is the stable integration surface for Goddard platform capabilities.

## Related Docs

- [SDK Glossary](./glossary.md)

## Package Surfaces

| Import | Owns | Does not own |
| --- | --- | --- |
| `@goddard-ai/sdk` | Daemon-backed auth | Direct backend HTTP ownership, PR operations, stream subscription |
| `@goddard-ai/sdk/daemon` | Daemon-backed agent sessions, explicit loop lifecycle control, and workforce operations | Low-level daemon URL parsing and IPC transport factories |
| `@goddard-ai/sdk/node` | Node-side daemon defaults for auth, actions, loops, and workforce | Local config loading, repo initialization ownership |

## `daemon-client` vs `sdk/daemon`

Use `@goddard-ai/daemon-client` when you need to:

- Parse or construct daemon URLs.
- Bind a host-specific IPC client implementation such as Node sockets or Tauri IPC.

Use `@goddard-ai/daemon-client/node` when you need to:

- Resolve daemon connection settings from environment variables.
- Use the default Node socket transport.

Use `@goddard-ai/sdk/daemon` when you need to:

- Create or reconnect daemon-backed agent sessions.
- Send prompts through ACP.
- Read daemon-managed session history.
- Shut sessions down through the daemon contract.
- Start, inspect, list, or stop daemon-managed loops.
- Start, inspect, list, or stop daemon-managed workforce runtimes.

Fresh daemon-backed sessions keep the original `cwd` by default, even inside git repositories. Set `worktree: { enabled: true }` to opt into isolated worktree execution for that session.

`daemon-client` owns transport setup. `sdk/daemon` owns daemon-facing higher-level semantics.

## Environment Behavior

`@goddard-ai/sdk/daemon` accepts explicit daemon connection options for non-Node hosts.

- Node convenience path: use `@goddard-ai/sdk/node` when you want env-based defaults.
- App path: pass an explicit client or an explicit `{ daemonUrl, createClient }` pair. Do not rely on Node defaults in the app.

## Configuration Ownership

The daemon resolves persisted JSON config, packaged actions, and packaged loops from disk. `@goddard-ai/sdk/node` does not read those files locally.

- Global defaults: `~/.goddard/config.json`
- Local defaults: `<repo>/.goddard/config.json`
- Prompt-only actions: `.goddard/actions/<name>.md`
- Packaged actions: `.goddard/actions/<name>/prompt.md` + `.goddard/actions/<name>/config.json`
- Packaged loops: `.goddard/loops/<name>/prompt.js` + `.goddard/loops/<name>/config.json`

## Examples

Top-level SDK auth usage:

```ts
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"
import { GoddardSdk } from "@goddard-ai/sdk"

const sdk = new GoddardSdk({
  client: createDaemonIpcClient({
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
    createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
  }),
})

const session = await sdk.auth.login({
  githubUsername: "alec",
  onPrompt(verificationUri, userCode) {
    console.log(`Open ${verificationUri} and enter ${userCode}`)
  },
})

const me = await sdk.auth.whoami()
console.log(me.githubUsername)
```

Daemon session client usage:

```ts
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"
import { runAgent } from "@goddard-ai/sdk/daemon"

const session = await runAgent(
  {
    agent: "pi",
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  },
  {
    client: createDaemonIpcClient({
      daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
      createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
    }),
  },
)

await session.prompt("Review the current diff.")
const history = await session.getHistory()
await session.stop()
```

Loop daemon lifecycle usage:

```ts
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"
import { startDaemonLoop } from "@goddard-ai/sdk/daemon"

const loop = await startDaemonLoop(
  {
    rootDir: process.cwd(),
    loopName: "triage",
    session: {
      cwd: process.cwd(),
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
    client: createDaemonIpcClient({
      daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
      createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
    }),
  },
)

console.log(loop.sessionId)
```

Node convenience usage:

```ts
import { GoddardSdk } from "@goddard-ai/sdk/node"

const sdk = new GoddardSdk()

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
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { runAgent } from "@goddard-ai/sdk/daemon"

await runAgent(
  {
    agent: "pi",
    cwd: "/workspace",
    mcpServers: [],
  },
  {
    client: createDaemonIpcClient({
      daemonUrl,
      createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
    }),
  },
)
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).

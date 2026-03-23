# `@goddard-ai/sdk`

`@goddard-ai/sdk` is the stable integration surface for daemon-backed Goddard capabilities.

## Related Docs

- [SDK Glossary](./glossary.md)

## Package Surfaces

| Import | Owns | Does not own |
| --- | --- | --- |
| `@goddard-ai/sdk` | Browser-safe daemon IPC methods exposed through one SDK instance | Host-specific daemon URL defaults and socket transport setup |
| `@goddard-ai/sdk/daemon` | Stable daemon session helpers that return an object-backed live agent session | Root-level namespace wrappers for non-session daemon IPC |
| `@goddard-ai/sdk/node` | The same SDK surface with Node daemon-client injection | Local config loading or extra Node-only wrapper methods |

## Relationship To `daemon-client`

Use `@goddard-ai/daemon-client` when you need to:

- Parse or construct daemon URLs.
- Bind a host-specific IPC client implementation such as Tauri IPC.

Use `@goddard-ai/daemon-client/node` when you need to:

- Resolve daemon connection settings from environment variables.
- Use the default Node socket transport.

Use `@goddard-ai/sdk` when you need to:

- Call daemon IPC actions through one stable SDK instance.
- Work from a browser-safe or Tauri host with an explicit daemon client.
- Use the same auth, PR, session, action, loop, and workforce method shapes as other hosts.

Use `@goddard-ai/sdk/daemon` when you need to:

- Create or reconnect one live daemon-backed agent session.
- Keep a stable `AgentSession` object for prompts, cancellation, history, shutdown, and model changes.
- Avoid rebuilding ACP transport wiring in each caller.

Use `@goddard-ai/sdk/node` when you need to:

- Reuse the browser-safe SDK surface from Node.
- Inject the Node daemon client automatically.

## API Shape

- The SDK mirrors the daemon IPC contract through namespace getters.
- Each namespace method takes one plain object payload.
- Each namespace method exposes the daemon response shape directly.
- The namespace getters are cached after first access.

Namespaces:

- `sdk.daemon`
- `sdk.auth`
- `sdk.pr`
- `sdk.session`
- `sdk.action`
- `sdk.loop`
- `sdk.workforce`

## Examples

Browser-safe explicit client:

```ts
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { GoddardSdk } from "@goddard-ai/sdk"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"

const sdk = new GoddardSdk({
  client: createDaemonIpcClient({
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
    createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
  }),
})

const authSession = await sdk.auth.startDeviceFlow({
  githubUsername: "alec",
})

const me = await sdk.auth.whoami({})
const loop = await sdk.loop.get({
  rootDir: "/workspace",
  loopName: "triage",
})
```

Node usage:

```ts
import { GoddardSdk } from "@goddard-ai/sdk/node"

const sdk = new GoddardSdk()

const started = await sdk.workforce.start({
  rootDir: process.cwd(),
})

const listed = await sdk.workforce.list({})

await sdk.workforce.request({
  rootDir: started.workforce.rootDir,
  targetAgentId: started.workforce.config.rootAgentId,
  input: "Review the current diff.",
})

console.log(listed.workforces.length)
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).

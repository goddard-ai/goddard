# `@goddard-ai/sdk`

`@goddard-ai/sdk` is the stable integration surface for daemon-backed Goddard capabilities.

## Related Docs

- [SDK Glossary](./glossary.md)

## Package Surfaces

| Import                 | Owns                                                             | Does not own                                                 |
| ---------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------ |
| `@goddard-ai/sdk`      | Browser-safe daemon IPC methods exposed through one SDK instance | Host-specific daemon URL defaults and socket transport setup |
| `@goddard-ai/sdk/node` | The same SDK surface with Node daemon-client injection           | Local config loading or extra Node-only wrapper methods      |

## Relationship To `daemon-client`

Use `@goddard-ai/daemon-client` when you need to:

- Parse or construct daemon URLs.
- Bind a host-specific IPC client implementation.

Use `@goddard-ai/daemon-client/node` when you need to:

- Resolve daemon connection settings from environment variables.
- Use the default Node socket transport.

Use `@goddard-ai/sdk` when you need to:

- Call daemon IPC actions through one stable SDK instance.
- Work from a browser-safe or other non-Node host with an explicit daemon client.
- Use the same auth, PR, session, action, loop, and workforce method shapes as other hosts.
- Create or reconnect one live daemon-backed agent session through `sdk.session.run(...)`.
- Keep a stable `AgentSession` object for prompts, daemon-owned turn cancellation, steering, history, shutdown, and model changes.
- Subscribe to live daemon-filtered session updates through `sdk.session.subscribe(...)`.

Use `@goddard-ai/sdk/node` when you need to:

- Reuse the browser-safe SDK surface from Node.
- Inject the Node daemon client automatically.

## API Shape

- The SDK mirrors the daemon IPC contract through namespace getters.
- `sdk.session.run(...)` is the object-backed exception used for live agent session interaction.
- `sdk.session.subscribe(...)` is the callback-based exception used for live daemon session updates.
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
import { createClient } from "@goddard-ai/ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { GoddardSdk } from "@goddard-ai/sdk"

const desktopHost = globalThis.desktopHost

const sdk = new GoddardSdk({
  client: createClient(daemonIpcSchema, {
    send(name, payload) {
      return desktopHost.send({
        socketPath: "/tmp/goddard-daemon.sock",
        name,
        payload,
      })
    },
    subscribe(name, filter, onMessage) {
      return desktopHost.subscribe(
        {
          socketPath: "/tmp/goddard-daemon.sock",
          name,
          filter,
        },
        onMessage,
      )
    },
  }),
})

const authSession = await sdk.auth.startDeviceFlow({
  githubUsername: "alec",
})

const me = await sdk.auth.whoami()
const loop = await sdk.loop.get({
  rootDir: "/workspace",
  loopName: "triage",
})
const unsubscribe = await sdk.session.subscribe({ id: "session-1" }, (message) => {
  console.log(message)
})

unsubscribe()
```

Node usage:

```ts
import { GoddardSdk } from "@goddard-ai/sdk/node"

const sdk = new GoddardSdk()

const started = await sdk.workforce.start({
  rootDir: process.cwd(),
})

const listed = await sdk.workforce.list()

await sdk.workforce.request({
  rootDir: started.workforce.rootDir,
  targetAgentId: started.workforce.config.rootAgentId,
  input: "Review the current diff.",
})

console.log(listed.workforces.length)
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).

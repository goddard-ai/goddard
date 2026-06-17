# `@goddard-ai/sdk`

`@goddard-ai/sdk` is the stable integration surface for daemon-backed Goddard capabilities.

## Related Docs

- [SDK Glossary](./glossary.md)
- [Daemon-Backed ACP Session Bridge](./acp-session-bridge.md)

## Package Surfaces

| Import                 | Owns                                                             | Does not own                                              |
| ---------------------- | ---------------------------------------------------------------- | --------------------------------------------------------- |
| `@goddard-ai/sdk`      | Browser-safe daemon IPC methods exposed through one SDK instance | Host-specific daemon URL defaults and TCP transport setup |
| `@goddard-ai/sdk/node` | The same SDK surface with Node daemon-client injection           | Local config loading or extra Node-only wrapper methods   |

## Relationship To `daemon-client`

Use `@goddard-ai/daemon-client` when you need to:

- Parse or construct daemon URLs.
- Bind a host-specific IPC client implementation.

Use `@goddard-ai/daemon-client/node` when you need to:

- Resolve daemon connection settings from environment variables.
- Use the default Node TCP transport.

Use `@goddard-ai/sdk` when you need to:

- Call daemon IPC actions through one stable SDK instance.
- Work from a browser-safe or other non-Node host with an explicit daemon client.
- Use the same auth, PR, session, action, loop, and workforce method shapes as other hosts.
- Create or reconnect one live daemon-backed agent session through `sdk.session.run(...)`.
- Keep a stable `AgentSession` object for prompts, daemon-owned turn cancellation, steering, history, shutdown, and model changes.
- Stream live daemon-filtered session updates through generated stream routes such as
  `sdk.session.streamMessages(...)`.

Use `@goddard-ai/sdk/node` when you need to:

- Reuse the browser-safe SDK surface from Node.
- Inject the Node daemon client automatically.

## API Shape

- The SDK mirrors the daemon IPC contract through namespace getters.
- Feature-owned SDK namespaces are contributed by internal packages under `features/<name>/src/sdk.ts` and then bundled by this public SDK composition root.
- `sdk.session.run(...)` is the object-backed exception used for live agent session interaction.
- Generated stream routes return async iterables and use `AbortSignal` cancellation.
- Each namespace method takes one plain object payload.
- Each namespace method exposes the daemon response shape directly.
- The namespaces are assigned when the SDK instance is constructed.

Namespaces:

- `sdk.daemon`
- `sdk.auth`
- `sdk.pr`
- `sdk.inbox`
- `sdk.session`
- `sdk.action`
- `sdk.loop`
- `sdk.pipeline`
- `sdk.workforce`

## Pipeline Namespace

Use `sdk.pipeline` to control daemon-managed Pipeline runs:

```ts
const definitions = await sdk.pipeline.listDefinitions({
  cwd: process.cwd(),
})

const spawned = await sdk.pipeline.spawnRun({
  cwd: process.cwd(),
  pipelineId: "creative-weaver",
  inputs: {
    premise: "A lighthouse keeper ends a long friendship before dawn",
    emotion: "grief",
    seed: 144,
    targetWords: 500,
  },
  origin: "sdk",
  visibility: "visible",
})

const advanced = await sdk.pipeline.advanceRun({
  id: spawned.run.id,
})

console.log(definitions.definitions.length, advanced.run.status)
```

The namespace also lists runs, reads one run with ordered steps, cancels cancellable runs, retries failed runs, and approves waiting approval steps.

## Feature Composition

Feature packages do not import `@goddard-ai/sdk`. They import
`@goddard-ai/sdk-plugin` and export a feature SDK plugin, usually from
`features/<name>/src/sdk.ts`. This package imports those feature SDK plugins and
attaches their namespaces to `GoddardSdk`.

`features/inbox` is the reference SDK feature package. Its `inboxSdkPlugin`
preserves the existing `sdk.inbox` namespace while moving the namespace factory
out of the central SDK file.

## Examples

Browser-safe explicit client:

```ts
import { daemonIpcRoutes } from "@goddard-ai/daemon-client/daemon-ipc"
import { createRouteClient, ndjson } from "@goddard-ai/ipc"
import { GoddardSdk } from "@goddard-ai/sdk"

const desktopHost = globalThis.desktopHost

const sdk = new GoddardSdk({
  client: createRouteClient({
    baseURL: "http://127.0.0.1:49827/",
    routes: daemonIpcRoutes,
    plugins: [ndjson.clientPlugin],
    fetch: desktopHost.fetch,
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
const abortController = new AbortController()
const messages = await sdk.session.streamMessages(
  { id: "session-1" },
  { signal: abortController.signal },
)
for await (const message of messages) {
  console.log(message)
}
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

This project is licensed under the [Apache License 2.0](./LICENSE).

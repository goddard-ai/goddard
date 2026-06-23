# `@goddard-ai/daemon-client`

Low-level daemon connection helpers shared by Node, the app, and SDK composition layers.

## Package Surfaces

- `@goddard-ai/daemon-client`
  - Shared daemon IPC client types only.
- `@goddard-ai/daemon-client/node`
  - Node env/default helpers, the default TCP transport, and the generated daemon IPC command tree.
- `@goddard-ai/daemon-client/browser`
  - Browser Fetch transport for direct loopback daemon IPC with bearer-token authorization.
- `@goddard-ai/daemon-client/daemon-ipc`
  - The composed default daemon IPC route contract.

Use `@goddard-ai/daemon-client` when you need to:

- Type an injected daemon IPC client or client factory.

Use `@goddard-ai/daemon-client/node` when you need to:

- Create a daemon IPC client from an explicit daemon URL.
- Create the default Node TCP client from env/default settings.
- Create the generated daemon IPC command tree for an operational CLI shell.

Use `@goddard-ai/daemon-client/browser` when you need to:

- Create a browser-safe daemon IPC client from an explicit loopback daemon URL.
- Resolve browser daemon access lazily from a host or pairing layer.
- Attach a hosted-browser pairing token or desktop webview token to direct daemon requests.
- Consume daemon NDJSON stream routes through browser Fetch.

Use `@goddard-ai/sdk` for explicit browser-safe daemon calls, or `@goddard-ai/sdk/node` when you want the same SDK surface with Node daemon-client injection.

```ts
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"

const client = createDaemonIpcClient({
  daemonUrl: "http://127.0.0.1:49827/",
})
```

```ts
import { createBrowserDaemonIpcClient } from "@goddard-ai/daemon-client/browser"

const client = createBrowserDaemonIpcClient({
  daemonUrl: "http://127.0.0.1:49827/",
  token: () => localStorage.getItem("goddard.daemonBrowserToken"),
})
```

```ts
import { createBrowserDaemonIpcClient } from "@goddard-ai/daemon-client/browser"

const client = createBrowserDaemonIpcClient({
  access: () => window.__goddardDesktop.createDaemonWebviewAccessToken(window.location.origin),
})
```

Browser access is disabled by default at the daemon boundary and requires exact origin allowlisting plus either hosted-browser pairing or desktop webview token bootstrap. See [Browser Access](../docs/concepts/browser-access.md) for the operating model and troubleshooting notes.

## Generated IPC Shell

The Node package exposes a generated `cmd-ts` command tree for the daemon IPC surface. The command tree is derived from the composed daemon route contract, so supported commands follow the same daemon capabilities as SDK and daemon clients instead of maintaining a separate command inventory.

The generated shell is an operator and maintainer surface for direct local inspection or recovery. It is not a replacement for the app, SDK integrations, or curated operational workflows.

Request-bearing routes accept one JSON payload:

```text
goddard ipc session get --json '{"id":"ses_..."}'
```

Calling a request-bearing route without `--json` prints the expected JSON shape instead of contacting the daemon:

```text
goddard ipc session get
{
  "id": "<value>"
}
```

Routes without request payloads take no payload:

```text
goddard ipc daemon health
```

Non-stream routes print one JSON response. Stream routes print one JSON object per line until the stream ends or the process is interrupted.

The command validates request payloads with the same runtime schemas used by daemon IPC. Response shapes remain compile-time contracts; the generated shell does not runtime-validate responses.

First-class scalar flags for simple request fields may be added later, but JSON input remains the complete form for the generated surface.

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).

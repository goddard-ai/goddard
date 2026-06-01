# `@goddard-ai/daemon-client`

Low-level daemon connection helpers shared by Node, the app, and SDK composition layers.

## Package Surfaces

- `@goddard-ai/daemon-client`
  - Shared daemon IPC client types only.
- `@goddard-ai/daemon-client/node`
  - Node env/default helpers and the default TCP transport.
- `@goddard-ai/daemon-client/daemon-ipc`
  - The composed default daemon IPC route contract.

Use `@goddard-ai/daemon-client` when you need to:

- Type an injected daemon IPC client or client factory.

Use `@goddard-ai/daemon-client/node` when you need to:

- Create a daemon IPC client from an explicit daemon URL.
- Create the default Node TCP client from env/default settings.

Use `@goddard-ai/sdk` for explicit browser-safe daemon calls, or `@goddard-ai/sdk/node` when you want the same SDK surface with Node daemon-client injection.

```ts
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"

const client = createDaemonIpcClient({
  daemonUrl: "http://127.0.0.1:49827/",
})
```

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).

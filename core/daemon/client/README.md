# `@goddard-ai/daemon-client`

Low-level daemon connection helpers shared by Node, the app, and SDK composition layers.

## Package Surfaces

- `@goddard-ai/daemon-client`
  - Explicit daemon URL parsing and host-injected client creation.
- `@goddard-ai/daemon-client/node`
  - Node env/default helpers and the default socket transport.

Use `@goddard-ai/daemon-client` when you need to:

- Create a daemon IPC client with a host-specific transport factory.
- Parse or construct the daemon URL format.

Use `@goddard-ai/daemon-client/node` when you need to:

- Resolve `GODDARD_DAEMON_URL`.
- Derive `GODDARD_DAEMON_URL` from `GODDARD_DAEMON_SOCKET_PATH`.
- Create the default Node socket client.

Use `@goddard-ai/sdk` for explicit browser-safe daemon calls, or `@goddard-ai/sdk/node` when you want the same SDK surface with Node daemon-client injection.

```ts
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"

const client = createDaemonIpcClient({
  daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
  createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
})
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).

# `@goddard-ai/daemon-client`

Low-level daemon connection helpers shared by Node, the app, and SDK composition layers.

Use this package when you need to:

- Resolve `GODDARD_DAEMON_URL`.
- Derive `GODDARD_DAEMON_URL` from `GODDARD_DAEMON_SOCKET_PATH` when the launcher passes only a socket path.
- Create a daemon IPC client with a host-specific transport factory.
- Parse or construct the daemon URL format.

Use `@goddard-ai/sdk/daemon` instead when you need daemon-backed agent sessions and ACP prompt/history/shutdown helpers.

```ts
import { createDaemonIpcClient, resolveDaemonConnectionFromEnv } from "@goddard-ai/daemon-client"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"

const { daemonUrl } = resolveDaemonConnectionFromEnv()

const client = createDaemonIpcClient({
  daemonUrl,
  createClient: ({ socketPath }) => createTauriClient(socketPath, daemonIpcSchema),
})
```

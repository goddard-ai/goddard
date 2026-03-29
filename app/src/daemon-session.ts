import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import type { GoddardSdkOptions } from "@goddard-ai/sdk"
import { createTauriClient } from "@goddard-ai/tauri-plugin-ipc"

export function createAppDaemonIpcClient(daemonUrl: string) {
  return createDaemonIpcClient({
    daemonUrl,
    createClient: (input: { socketPath: string }) =>
      createTauriClient(input.socketPath, daemonIpcSchema),
  })
}

export function createAppDaemonSessionOptions(daemonUrl: string): GoddardSdkOptions {
  return {
    client: createAppDaemonIpcClient(daemonUrl),
  }
}

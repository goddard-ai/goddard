import type { DaemonIpcClient, DaemonIpcClientFactory } from "@goddard-ai/daemon-client"
import type { IpcClientHook } from "@goddard-ai/ipc"

/** Minimal daemon client contract accepted by the SDK. */
export type GoddardClient = DaemonIpcClient

/** Shared explicit connection options for SDK calls that talk to the daemon over IPC. */
export type IpcClientOptions =
  | {
      client: GoddardClient
    }
  | {
      daemonUrl: string
      ipcHook?: IpcClientHook
      createClient: DaemonIpcClientFactory<GoddardClient>
    }

/** Resolves the daemon IPC client from explicit browser-safe connection inputs. */
export function resolveIpcClient(options: IpcClientOptions): GoddardClient {
  if ("client" in options) {
    return options.client
  }
  const { createClient, daemonUrl, ipcHook } = options
  return createClient({
    daemonUrl,
    ipcHook,
  })
}

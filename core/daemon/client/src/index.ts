import { createClient } from "@goddard-ai/ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl, readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url"

/** Socket metadata passed to environment-specific IPC client factories. */
export type DaemonIpcClientFactoryInput = {
  socketPath: string
}

/** IPC client type shared by all daemon transport implementations. */
export type DaemonIpcClient = ReturnType<typeof createClient<typeof daemonIpcSchema>>

/** Injectable factory for hosts that provide a custom IPC transport. */
export type DaemonIpcClientFactory<TClient = DaemonIpcClient> = (
  input: DaemonIpcClientFactoryInput,
) => TClient

export { createDaemonUrl, readSocketPathFromDaemonUrl }

/** Creates one daemon IPC client using an explicit daemon URL and host transport factory. */
export function createDaemonIpcClient<TClient = DaemonIpcClient>(options: {
  daemonUrl: string
  createClient: DaemonIpcClientFactory<TClient>
}): TClient {
  return options.createClient({
    socketPath: readSocketPathFromDaemonUrl(options.daemonUrl),
  })
}

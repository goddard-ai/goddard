/** Shared daemon IPC client types used by runtime-specific daemon client modules. */
import type { createClient } from "@goddard-ai/ipc"
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"

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

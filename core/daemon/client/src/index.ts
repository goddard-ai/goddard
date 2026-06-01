/** Shared daemon IPC client types used by runtime-specific daemon client modules. */
import type { RouzerClient } from "@goddard-ai/ipc"

import type { daemonIpcRoutes } from "./daemon-ipc.ts"

/** Daemon connection metadata passed to environment-specific IPC client factories. */
export type DaemonIpcClientFactoryInput = {
  daemonUrl: string
}

/** IPC client type shared by all daemon transport implementations. */
export type DaemonIpcClient = RouzerClient<typeof daemonIpcRoutes>

/** Injectable factory for hosts that provide a custom IPC transport. */
export type DaemonIpcClientFactory<TClient = DaemonIpcClient> = (
  input: DaemonIpcClientFactoryInput,
) => TClient

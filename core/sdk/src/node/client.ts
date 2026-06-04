import {
  createDaemonIpcClient,
  createDaemonIpcClientFromEnv,
  type DaemonClientEnv,
  type DaemonIpcClient,
  type DaemonIpcClientFactory,
} from "@goddard-ai/daemon-client/node"
import type { IpcClientHook } from "@goddard-ai/ipc"

import { resolveIpcClient, type GoddardClient } from "../ipc-client.ts"

/** Node-side daemon connection options, including env-driven defaults. */
export type NodeDaemonClientOptions = {
  client?: GoddardClient
  daemonUrl?: string
  ipcHook?: IpcClientHook
  createClient?: DaemonIpcClientFactory
  env?: DaemonClientEnv
}

/** Resolves the daemon IPC client from explicit Node options or env/default fallbacks. */
export function resolveNodeDaemonClient(options: NodeDaemonClientOptions = {}): DaemonIpcClient {
  if (options.client) {
    return resolveIpcClient({
      client: options.client,
    })
  }

  if (options.daemonUrl) {
    return createDaemonIpcClient({
      daemonUrl: options.daemonUrl,
      ipcHook: options.ipcHook,
      createClient: options.createClient,
    })
  }

  return createDaemonIpcClientFromEnv({
    env: options.env,
    ipcHook: options.ipcHook,
    createClient: options.createClient,
  }).client
}

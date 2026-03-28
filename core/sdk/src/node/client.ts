import type {
  DaemonClientEnv,
  DaemonIpcClient,
  DaemonIpcClientFactory,
} from "@goddard-ai/daemon-client/node"
import { createDaemonIpcClient, createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client/node"

/** Node-side daemon connection options, including env-driven defaults. */
export type NodeDaemonClientOptions = {
  client?: DaemonIpcClient
  daemonUrl?: string
  createClient?: DaemonIpcClientFactory
  env?: DaemonClientEnv
}

/** Resolves the daemon IPC client from explicit Node options or env/default fallbacks. */
export function resolveNodeDaemonClient(options: NodeDaemonClientOptions = {}): DaemonIpcClient {
  if (options.client) {
    return options.client
  }

  if (options.daemonUrl) {
    return createDaemonIpcClient({
      daemonUrl: options.daemonUrl,
      createClient: options.createClient,
    })
  }

  return createDaemonIpcClientFromEnv({
    env: options.env,
    createClient: options.createClient,
  }).client
}

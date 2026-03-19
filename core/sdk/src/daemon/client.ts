import type {
  DaemonClientEnv,
  DaemonIpcClient,
  DaemonIpcClientFactory,
} from "@goddard-ai/daemon-client"
import { createDaemonIpcClient, createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client"

/** Shared connection options for SDK helpers that talk to the daemon over IPC. */
// Options used by SDK daemon helpers to resolve an IPC client.
export type DaemonClientOptions = {
  client?: DaemonIpcClient
  daemonUrl?: string
  createClient?: DaemonIpcClientFactory
  env?: DaemonClientEnv
}

/** Resolves the daemon IPC client from an explicit client, daemon URL, or environment. */
export function resolveDaemonClient(options?: DaemonClientOptions): DaemonIpcClient {
  if (options?.client) {
    return options.client
  }

  if (options?.daemonUrl) {
    return createDaemonIpcClient({
      daemonUrl: options.daemonUrl,
      createClient: options.createClient,
    })
  }

  return createDaemonIpcClientFromEnv({
    env: options?.env,
    createClient: options?.createClient,
  }).client
}

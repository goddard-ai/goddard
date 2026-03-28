import type { DaemonIpcClient, DaemonIpcClientFactory } from "@goddard-ai/daemon-client"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"

/** Reuses one already constructed daemon IPC client for browser-safe SDK calls. */
export type InjectedDaemonClientOptions = {
  client: DaemonIpcClient
  daemonUrl?: never
  createClient?: never
}

/** Builds one daemon IPC client from an explicit daemon URL and host-specific transport factory. */
export type ExplicitDaemonClientFactoryOptions = {
  client?: never
  daemonUrl: string
  createClient: DaemonIpcClientFactory
}

/** Shared explicit connection options for SDK helpers that talk to the daemon over IPC. */
export type DaemonClientOptions = InjectedDaemonClientOptions | ExplicitDaemonClientFactoryOptions

/** Resolves the daemon IPC client from explicit browser-safe connection inputs. */
export function resolveDaemonClient(options: DaemonClientOptions): DaemonIpcClient {
  if ("client" in options && options.client !== undefined) {
    return options.client
  }

  return createDaemonIpcClient({
    daemonUrl: options.daemonUrl,
    createClient: options.createClient,
  })
}

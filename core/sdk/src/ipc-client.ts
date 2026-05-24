import type { DaemonIpcClientFactory } from "@goddard-ai/daemon-client"

/** Minimal daemon client contract accepted by the SDK without generic method signatures. */
export type GoddardClient = {
  readonly [namespace: string]: any
  readonly send: (name: string, payload?: any) => Promise<any>
  readonly subscribe: (target: any, onMessage: (payload: any) => void) => Promise<() => void>
}

/** Shared explicit connection options for SDK calls that talk to the daemon over IPC. */
export type IpcClientOptions =
  | {
      client: GoddardClient
    }
  | {
      daemonUrl: string
      createClient: DaemonIpcClientFactory<GoddardClient>
    }

/** Resolves the daemon IPC client from explicit browser-safe connection inputs. */
export function resolveIpcClient(options: IpcClientOptions): GoddardClient {
  if ("client" in options) {
    return options.client
  }
  const { createClient, daemonUrl } = options
  return createClient({
    daemonUrl,
  })
}

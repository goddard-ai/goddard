/** Shared daemon IPC client types used by runtime-specific daemon client modules. */

/** Daemon connection metadata passed to environment-specific IPC client factories. */
export type DaemonIpcClientFactoryInput = {
  daemonUrl: string
}

/** IPC client type shared by all daemon transport implementations. */
export type DaemonIpcClient = {
  readonly [namespace: string]: any
  readonly send: (name: string, payload?: any) => Promise<any>
  readonly subscribe: (target: any, onMessage: (payload: any) => void) => Promise<() => void>
}

/** Injectable factory for hosts that provide a custom IPC transport. */
export type DaemonIpcClientFactory<TClient = DaemonIpcClient> = (
  input: DaemonIpcClientFactoryInput,
) => TClient

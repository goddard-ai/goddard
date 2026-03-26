/**
 * Defines the transport layer interface for an IPC client.
 */
export type IpcTransport = {
  send(name: string, payload: unknown): Promise<unknown>
  subscribe(name: string, onMessage: (payload: unknown) => void): Promise<() => void> | (() => void)
}

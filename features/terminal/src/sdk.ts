import type {
  TerminalCloseRequest,
  TerminalConnectRequest,
  TerminalConnectResponse,
  TerminalCreateRequest,
  TerminalCreateResponse,
  TerminalDaemonEvent,
  TerminalDisconnectRequest,
  TerminalInputRequest,
  TerminalResizeRequest,
  TerminalRestartRequest,
} from "@goddard-ai/schema/daemon/terminals"
import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { terminalIpcRoutes } from "./daemon-ipc.ts"

/** Callback shape used by clients to receive daemon terminal lifecycle events in stream order. */
export type TerminalEventHandler = (event: TerminalDaemonEvent) => void | Promise<void>

/** Callback invoked when a terminal stream ends without an explicit client stop. */
export type TerminalStreamEndHandler = (error?: unknown) => void

/** Awaitable terminal stream teardown. */
export type StopTerminalSubscription = () => Promise<void>

/** SDK input for writing terminal data through the request surface. */
export type TerminalWriteInput = TerminalInputRequest

/** One SDK terminal connection that owns connection-local terminal instances. */
export interface GoddardTerminalConnection {
  readonly connectionId: TerminalConnectResponse["connectionId"]
  create(input: Omit<TerminalCreateRequest, "connectionId">): Promise<TerminalCreateResponse>
  write(input: Omit<TerminalInputRequest, "connectionId">): Promise<void>
  resize(input: Omit<TerminalResizeRequest, "connectionId">): Promise<void>
  restart(input: Omit<TerminalRestartRequest, "connectionId">): Promise<TerminalCreateResponse>
  close(input: Omit<TerminalCloseRequest, "connectionId">): Promise<void>
  disconnect(): Promise<void>
  subscribe(
    handler: TerminalEventHandler,
    onEnd: TerminalStreamEndHandler,
  ): Promise<StopTerminalSubscription>
}

/** SDK namespace for daemon-managed terminal instances. */
export type GoddardTerminalNamespace = {
  connect(input?: TerminalConnectRequest): Promise<GoddardTerminalConnection>
  create(input: TerminalCreateRequest): Promise<TerminalCreateResponse>
  write(input: TerminalWriteInput): Promise<void>
  resize(input: TerminalResizeRequest): Promise<void>
  restart(input: TerminalRestartRequest): Promise<TerminalCreateResponse>
  close(input: TerminalCloseRequest): Promise<void>
  disconnect(input: TerminalDisconnectRequest): Promise<void>
  subscribe(
    input: TerminalDisconnectRequest,
    handler: TerminalEventHandler,
    onEnd: TerminalStreamEndHandler,
  ): Promise<StopTerminalSubscription>
}

export const terminalSdkPlugin = defineSdkPlugin({
  name: "terminal",
  ipcRoutes: terminalIpcRoutes,
  wrap({ client }) {
    function createTerminalConnection(connectionId: string): GoddardTerminalConnection {
      let pendingOperation = Promise.resolve()

      function enqueue<T>(operation: () => Promise<T>) {
        const result = pendingOperation.then(operation)
        pendingOperation = result.then(
          () => undefined,
          () => undefined,
        )
        return result
      }

      return {
        connectionId,
        create: (input) => enqueue(() => client.terminal.create({ ...input, connectionId })),
        write: (input) =>
          enqueue(async () => {
            await client.terminal.write({ ...input, connectionId })
          }),
        resize: (input) =>
          enqueue(async () => {
            await client.terminal.resize({ ...input, connectionId })
          }),
        restart: (input) => enqueue(() => client.terminal.restart({ ...input, connectionId })),
        close: (input) =>
          enqueue(async () => {
            await client.terminal.close({ ...input, connectionId })
          }),
        disconnect: () =>
          enqueue(async () => {
            await client.terminal.disconnect({ connectionId })
          }),
        subscribe: async (handler, onEnd) => subscribe({ connectionId }, handler, onEnd),
      }
    }

    async function subscribe(
      input: TerminalDisconnectRequest,
      handler: TerminalEventHandler,
      onEnd: TerminalStreamEndHandler,
    ) {
      const controller = new AbortController()
      const events = await client.terminal.event(input, { signal: controller.signal })
      let stopped = false

      const consume = (async () => {
        let streamError: unknown
        try {
          for await (const event of events) {
            if (controller.signal.aborted) {
              break
            }
            await handler(event)
          }
        } catch (error) {
          streamError = error
        } finally {
          if (!stopped) {
            controller.abort()
            onEnd(streamError)
          }
        }
      })()

      return async () => {
        stopped = true
        controller.abort()
        await consume
      }
    }

    const terminal: GoddardTerminalNamespace = {
      connect: async (input = {}) => {
        const response = await client.terminal.connect(input)
        return createTerminalConnection(response.connectionId)
      },
      create: async (input) => client.terminal.create(input),
      write: async (input) => {
        await client.terminal.write(input)
      },
      resize: async (input) => {
        await client.terminal.resize(input)
      },
      restart: async (input) => client.terminal.restart(input),
      close: async (input) => {
        await client.terminal.close(input)
      },
      disconnect: async (input) => {
        await client.terminal.disconnect(input)
      },
      subscribe,
    }

    return {
      terminal,
    }
  },
})

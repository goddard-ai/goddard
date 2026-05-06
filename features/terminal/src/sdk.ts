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

/** Callback shape used by clients to receive daemon terminal lifecycle events. */
export type TerminalEventHandler = (event: TerminalDaemonEvent) => void

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
  subscribe(handler: TerminalEventHandler): Promise<() => void>
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
  subscribe(input: TerminalDisconnectRequest, handler: TerminalEventHandler): Promise<() => void>
}

export const terminalSdkPlugin = defineSdkPlugin({
  name: "terminal",
  ipcRoutes: terminalIpcRoutes,
  wrap({ client }) {
    function createTerminalConnection(connectionId: string): GoddardTerminalConnection {
      return {
        connectionId,
        create: async (input) => client.terminal.create({ ...input, connectionId }),
        write: async (input) => {
          await client.terminal.write({ ...input, connectionId })
        },
        resize: async (input) => {
          await client.terminal.resize({ ...input, connectionId })
        },
        restart: async (input) => client.terminal.restart({ ...input, connectionId }),
        close: async (input) => {
          await client.terminal.close({ ...input, connectionId })
        },
        disconnect: async () => {
          await client.terminal.disconnect({ connectionId })
        },
        subscribe: async (handler) => subscribe({ connectionId }, handler),
      }
    }

    async function subscribe(input: TerminalDisconnectRequest, handler: TerminalEventHandler) {
      const controller = new AbortController()
      const events = await client.terminal.event(input, { signal: controller.signal })

      void (async () => {
        try {
          for await (const event of events) {
            if (controller.signal.aborted) {
              break
            }
            handler(event)
          }
        } catch (error) {
          if (!controller.signal.aborted) {
            throw error
          }
        }
      })()

      return () => {
        controller.abort()
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

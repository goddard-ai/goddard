import { definePlugin } from "@goddard-ai/daemon-plugin"
import type {
  TerminalDaemonEvent,
  TerminalEventStreamFilter,
} from "@goddard-ai/schema/daemon/terminals"

import { terminalIpcRoutes } from "./daemon-ipc.ts"
import { DaemonTerminalConnectionRegistry } from "./daemon/connections.ts"
import { TerminalEventQueue } from "./daemon/event-queue.ts"
import { DaemonTerminalError } from "./daemon/runtime.ts"

export { runTerminalRuntimeCheck, type TerminalRuntimeCheckResult } from "./daemon/self-test.ts"
export { DaemonTerminalConnection, DaemonTerminalError } from "./daemon/runtime.ts"
export type { DaemonTerminalConnectionOptions } from "./daemon/runtime.ts"

export const terminalPlugin = definePlugin({
  name: "terminal",
  ipcRoutes: terminalIpcRoutes,
  setup() {
    const eventListeners = new Set<(event: TerminalDaemonEvent) => void>()
    const terminalConnections = new DaemonTerminalConnectionRegistry({
      publishEvent(event) {
        for (const listener of eventListeners) {
          listener(event)
        }
      },
    })

    function runTerminalRequest<T>(operation: () => T) {
      try {
        return operation()
      } catch (error) {
        terminalConnections.emitRequestError(error)
        if (error instanceof DaemonTerminalError) {
          throw new Error(error.message)
        }
        throw error
      }
    }

    async function* subscribeTerminalEvents(
      filter: TerminalEventStreamFilter,
      signal: AbortSignal,
    ) {
      const queue = new TerminalEventQueue()
      let wake: (() => void) | undefined
      const listener = (event: TerminalDaemonEvent) => {
        if (event.connectionId !== filter.connectionId) {
          return
        }
        queue.push(event)
        wake?.()
      }
      const abort = () => {
        wake?.()
      }

      terminalConnections.streamConnected(filter)
      eventListeners.add(listener)
      signal.addEventListener("abort", abort)
      try {
        while (!signal.aborted) {
          if (queue.overflowed) {
            throw new Error("Terminal event stream exceeded its buffer limit.")
          }

          const event = queue.shift()
          if (event) {
            yield event
            continue
          }
          await new Promise<void>((resolve) => {
            wake = resolve
          })
          wake = undefined
        }
      } finally {
        signal.removeEventListener("abort", abort)
        eventListeners.delete(listener)
        terminalConnections.streamDisconnected(filter)
      }
    }

    return {
      close: () => {
        terminalConnections.closeAll()
      },
      ipcHandlers: {
        terminal: {
          connect: async ({ body }) => terminalConnections.connect(body),
          create: async ({ body }) => runTerminalRequest(() => terminalConnections.create(body)),
          write: async ({ body }) => {
            runTerminalRequest(() => terminalConnections.write(body))
            return { success: true as const }
          },
          resize: async ({ body }) => {
            runTerminalRequest(() => terminalConnections.resize(body))
            return { success: true as const }
          },
          restart: async ({ body }) => runTerminalRequest(() => terminalConnections.restart(body)),
          close: async ({ body }) => {
            runTerminalRequest(() => terminalConnections.close(body))
            return { success: true as const }
          },
          disconnect: async ({ body }) => {
            runTerminalRequest(() => terminalConnections.disconnect(body))
            return { success: true as const }
          },
          event: (ctx) => {
            terminalConnections.requireConnection(ctx.query)
            return subscribeTerminalEvents(ctx.query, ctx.request.signal)
          },
        },
      },
    }
  },
})

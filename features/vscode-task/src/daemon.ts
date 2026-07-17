import { definePlugin } from "@goddard-ai/daemon-plugin"
import { terminalPlugin } from "@goddard-ai/terminal/daemon"

import { vscodeTaskIpcRoutes } from "./daemon-ipc.ts"
import { VscodeTaskManager } from "./daemon/manager.ts"
import type { VscodeTaskConnectionParams, VscodeTaskDaemonEvent } from "./schema.ts"

export { VscodeTaskManager, type VscodeTaskManagerOptions } from "./daemon/manager.ts"
export { createVscodeTaskHost, resolveTaskTerminalOptions } from "./daemon/host.ts"

export const vscodeTaskPlugin = definePlugin({
  name: "vscode-task",
  consumes: [terminalPlugin],
  ipcRoutes: vscodeTaskIpcRoutes,
  setup({ terminal }) {
    const eventListeners = new Set<(event: VscodeTaskDaemonEvent) => void>()
    const manager = new VscodeTaskManager({
      terminal,
      publishEvent(event) {
        for (const listener of eventListeners) {
          listener(event)
        }
      },
    })

    async function* subscribeEvents(filter: VscodeTaskConnectionParams, signal: AbortSignal) {
      const queue: VscodeTaskDaemonEvent[] = []
      let wake: (() => void) | undefined
      const listener = (event: VscodeTaskDaemonEvent) => {
        if (event.connectionId !== filter.connectionId) {
          return
        }
        queue.push(event)
        wake?.()
      }
      const abort = () => wake?.()

      manager.streamConnected(filter.connectionId)
      eventListeners.add(listener)
      signal.addEventListener("abort", abort)
      try {
        while (!signal.aborted) {
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
        manager.streamDisconnected(filter.connectionId)
      }
    }

    return {
      close: () => manager.closeAll(),
      ipcHandlers: {
        vscodeTask: {
          inspect: async ({ body }) => manager.inspect(body),
          connect: async () => manager.connect(),
          run: async ({ body }) => manager.run(body),
          cancel: async ({ body }) => manager.cancel(body),
          disconnect: async ({ body }) => manager.disconnect(body.connectionId),
          event: async function* (ctx) {
            yield* subscribeEvents(ctx.query, ctx.request.signal)
          },
        },
      },
    }
  },
})

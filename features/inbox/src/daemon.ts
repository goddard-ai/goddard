import { definePlugin } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { inboxIpcRoutes } from "./daemon-ipc.ts"
import { createInboxManager } from "./daemon/manager.ts"
import { inboxDbSchema } from "./daemon/store.ts"
import type { InboxItemEvent } from "./schema.ts"

export { createInboxManager, type InboxManager } from "./daemon/manager.ts"

export const inboxPlugin = definePlugin({
  name: "inbox",
  consumes: [sessionPlugin],
  db: inboxDbSchema,
  ipcRoutes: inboxIpcRoutes,
  setup({ db, session }) {
    const itemListeners = new Set<(event: InboxItemEvent) => void>()
    const inbox = createInboxManager({
      db,
      publishEvent: (payload) => {
        for (const listener of itemListeners) {
          listener(payload)
        }
      },
    })

    async function* subscribeInboxItems(signal: AbortSignal) {
      const queue: InboxItemEvent[] = []
      let wake: (() => void) | undefined
      const listener = (event: InboxItemEvent) => {
        queue.push(event)
        wake?.()
      }
      const abort = () => {
        wake?.()
      }

      itemListeners.add(listener)
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
        itemListeners.delete(listener)
      }
    }

    session.events.on("lifecycle.blocked", (event) => {
      inbox.touchInboxItem({
        entityId: event.sessionId,
        reason: "session.blocked",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    session.events.on("lifecycle.turnEnded", (event) => {
      inbox.touchInboxItem({
        entityId: event.sessionId,
        reason: "session.turn_ended",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    session.events.on("lifecycle.replied", (event) => {
      inbox.markSessionReplied(event.sessionId)
    })
    session.events.on("lifecycle.completed", (event) => {
      return inbox.completeSession(event.sessionId)
    })

    return {
      provides: {
        inbox: {
          touchInboxItem: inbox.touchInboxItem,
        },
      },
      ipcHandlers: {
        inbox: {
          list: async ({ body }) => inbox.listInboxItems(body),
          update: async ({ body }) => inbox.updateInboxItem(body),
          bulkUpdate: async ({ body }) => inbox.bulkUpdateInboxItems(body),
          item: async function* (ctx) {
            yield* subscribeInboxItems(ctx.request.signal)
          },
        },
      },
    }
  },
})

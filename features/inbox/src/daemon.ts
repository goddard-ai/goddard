import { definePlugin, type Plugin } from "@goddard-ai/daemon-plugin"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { inboxIpcRoutes } from "./daemon-ipc.ts"
import { createInboxManager } from "./daemon/manager.ts"
import { inboxDbSchema } from "./daemon/store.ts"
import type { InboxItemEvent } from "./schema.ts"

export { createInboxManager, type InboxManager } from "./daemon/manager.ts"

type InboxPlugin = Plugin & {
  readonly name: "inbox"
  readonly consumes: readonly [typeof sessionPlugin, typeof pullRequestPlugin]
  readonly db: typeof inboxDbSchema
  readonly ipcRoutes: typeof inboxIpcRoutes
}

const inboxPluginDefinition = definePlugin({
  name: "inbox",
  consumes: [sessionPlugin, pullRequestPlugin],
  db: inboxDbSchema,
  ipcRoutes: inboxIpcRoutes,
  setup({ db, events, session }) {
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

    events.on("session.blocked", (event) => {
      inbox.touchInboxItem({
        entityId: event.sessionId,
        reason: "session.blocked",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    events.on("session.turn.ended", (event) => {
      inbox.touchInboxItem({
        entityId: event.sessionId,
        reason: "session.turn_ended",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    events.on("session.replied", (event) => {
      inbox.markSessionReplied(event.sessionId)
    })
    events.on("pull_request.created", (event) => {
      inbox.touchInboxItem({
        entityId: event.pullRequestId,
        reason: "pull_request.created",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    events.on("pull_request.updated", (event) => {
      inbox.touchInboxItem({
        entityId: event.pullRequestId,
        reason: "pull_request.updated",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })

    return {
      ipcHandlers: {
        inbox: {
          list: async ({ body }) => inbox.listInboxItems(body),
          update: async ({ body }) => inbox.updateInboxItem(body),
          bulkUpdate: async ({ body }) => inbox.bulkUpdateInboxItems(body),
          completeSession: async ({ body: { id } }) => {
            await session.completeSession(id)
            return {
              item: inbox.completeSession(id),
            }
          },
          item: async function* (ctx) {
            yield* subscribeInboxItems(ctx.request.signal)
          },
        },
      },
    }
  },
})

export const inboxPlugin: InboxPlugin = inboxPluginDefinition

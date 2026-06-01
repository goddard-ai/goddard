import { definePlugin, type Plugin } from "@goddard-ai/daemon-plugin"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { inboxIpcRoutes } from "./daemon-ipc.ts"
import { createInboxManager } from "./daemon/manager.ts"
import { inboxDbSchema } from "./daemon/store.ts"
import type { InboxItemEvent } from "./schema.ts"

export { createInboxManager, type InboxManager } from "./daemon/manager.ts"

type InboxPlugin = {
  readonly name: "inbox"
  readonly consumes: readonly [typeof sessionPlugin, typeof pullRequestPlugin]
  readonly db: typeof inboxDbSchema
  readonly ipcRoutes: typeof inboxIpcRoutes
  readonly setup: Plugin["setup"]
}

export const inboxPlugin: InboxPlugin = definePlugin({
  name: "inbox",
  consumes: [sessionPlugin, pullRequestPlugin],
  db: inboxDbSchema,
  ipcRoutes: inboxIpcRoutes,
  setup({ db, pullRequest, session }) {
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
    pullRequest.events.on("lifecycle.created", (event) => {
      inbox.touchInboxItem({
        entityId: event.pullRequestId,
        reason: "pull_request.created",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    pullRequest.events.on("lifecycle.updated", (event) => {
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

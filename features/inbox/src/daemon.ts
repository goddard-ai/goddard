import { definePlugin } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { inboxIpcSchema } from "./daemon-ipc.ts"
import { createInboxManager, type InboxManager } from "./daemon/manager.ts"
import { inboxDbSchema } from "./daemon/store.ts"

export { createInboxManager, type InboxManager } from "./daemon/manager.ts"

/** First-class inbox methods exposed to daemon plugins that create attention items. */
type InboxExtension = Pick<InboxManager, "touchInboxItem">

export const inboxPlugin = definePlugin({
  name: "inbox",
  consumes: [sessionPlugin],
  db: inboxDbSchema,
  ipc: inboxIpcSchema,
  setup({ db, publish, session }) {
    const manager = createInboxManager({
      db,
      publishEvent: (payload) => {
        publish("inbox.item", payload)
      },
    })

    const inbox = {
      touchInboxItem: manager.touchInboxItem,
    } satisfies InboxExtension

    session.events.on("lifecycle.blocked", (event) => {
      manager.touchInboxItem({
        entityId: event.sessionId,
        reason: "session.blocked",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    session.events.on("lifecycle.turnEnded", (event) => {
      manager.touchInboxItem({
        entityId: event.sessionId,
        reason: "session.turn_ended",
        scope: event.scope,
        headline: event.headline,
        turnId: event.turnId,
      })
    })
    session.events.on("lifecycle.replied", (event) => {
      manager.markSessionReplied(event.sessionId)
    })
    session.events.on("lifecycle.completed", (event) => {
      return manager.completeSession(event.sessionId)
    })

    return {
      provides: {
        inbox,
      },
      requestHandlers: {
        "inbox.list": async (payload) => manager.listInboxItems(payload),
        "inbox.update": async (payload) => manager.updateInboxItem(payload),
        "inbox.bulkUpdate": async (payload) => manager.bulkUpdateInboxItems(payload),
      },
    }
  },
})

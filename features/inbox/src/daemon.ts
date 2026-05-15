import { definePlugin } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { inboxIpcSchema } from "./daemon-ipc.ts"
import { createInboxManager, type InboxManager } from "./daemon/manager.ts"

export { createInboxManager, type InboxManager } from "./daemon/manager.ts"

/** First-class inbox methods exposed to daemon plugins that create attention items. */
type InboxExtension = Pick<
  InboxManager,
  | "touchInboxItem"
  | "markSessionReplied"
  | "completeSession"
  | "getPullRequest"
  | "listInboxItems"
  | "updateInboxItem"
  | "bulkUpdateInboxItems"
>

export const inboxPlugin = definePlugin({
  name: "inbox",
  consumes: [sessionPlugin],
  ipc: inboxIpcSchema,
  setup({ publish, session }) {
    const inbox = createInboxManager({
      publishEvent: (payload) => {
        publish("inbox.item", payload)
      },
    }) satisfies InboxExtension

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
        inbox,
      },
      requestHandlers: {
        "inbox.list": async (payload) => inbox.listInboxItems(payload),
        "inbox.update": async (payload) => inbox.updateInboxItem(payload),
        "inbox.bulkUpdate": async (payload) => inbox.bulkUpdateInboxItems(payload),
      },
    }
  },
})

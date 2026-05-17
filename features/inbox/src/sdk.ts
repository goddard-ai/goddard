import { defineRequest, defineSdkPlugin, defineSubscription } from "@goddard-ai/sdk-plugin"

import { inboxIpcSchema } from "./daemon-ipc.ts"

export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  ipc: inboxIpcSchema,
  create({ client }) {
    return {
      inbox: {
        /** Lists daemon-local inbox rows using daemon ordering and filtering. */
        list: defineRequest(client, "inbox.list"),

        /** Updates one daemon-local inbox row by entity id. */
        update: defineRequest(client, "inbox.update"),

        /** Updates many daemon-local inbox rows with one shared daemon timestamp. */
        bulkUpdate: defineRequest(client, "inbox.bulkUpdate"),

        /** Subscribes to daemon-published inbox item updates. */
        subscribe: defineSubscription(client, "inbox.item"),
      },
    }
  },
})

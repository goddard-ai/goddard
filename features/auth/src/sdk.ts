import { defineRequest, defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { authIpcSchema } from "./daemon-ipc.ts"

export const authSdkPlugin = defineSdkPlugin({
  name: "auth",
  ipc: authIpcSchema,
  create({ client }) {
    return {
      auth: {
        /** Starts one GitHub device flow through the daemon auth contract. */
        startDeviceFlow: defineRequest(client, "auth.device.start"),

        /** Completes one pending GitHub device flow through the daemon auth contract. */
        completeDeviceFlow: defineRequest(client, "auth.device.complete"),

        /** Reads the current daemon-owned auth session as-is. */
        whoami: defineRequest(client, "auth.whoami"),

        /** Clears the current daemon-owned auth session as-is. */
        logout: defineRequest(client, "auth.logout"),
      },
    }
  },
})

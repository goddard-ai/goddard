import { definePlugin } from "@goddard-ai/daemon-plugin"

import { authIpcSchema } from "./daemon-ipc.ts"

export const authPlugin = definePlugin({
  name: "auth",
  ipc: authIpcSchema,
  setup({ authBackendClient, authTokenStore }) {
    return {
      requestHandlers: {
        "auth.device.start": async (payload) => authBackendClient.startDeviceFlow(payload),
        "auth.device.complete": async (payload) => {
          const session = await authBackendClient.completeDeviceFlow(payload)
          await authTokenStore.set(session.token)
          return session
        },
        "auth.whoami": async () => authBackendClient.whoami(),
        "auth.logout": async () => {
          await authBackendClient.logout()
          await authTokenStore.delete()
          return { success: true as const }
        },
      },
    }
  },
})

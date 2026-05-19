import { definePlugin } from "@goddard-ai/daemon-plugin"

import { authIpcSchema } from "./daemon-ipc.ts"

export const authPlugin = definePlugin({
  name: "auth",
  ipc: authIpcSchema,
  setup({ authTokenStore, backendClient }) {
    return {
      requestHandlers: {
        "auth.device.start": async (payload) => backendClient.auth.startDeviceFlow(payload),
        "auth.device.complete": async (payload) => {
          const session = await backendClient.auth.completeDeviceFlow(payload)
          await authTokenStore.set(session.token)
          return session
        },
        "auth.whoami": async () => backendClient.auth.whoami(),
        "auth.logout": async () => {
          await backendClient.auth.logout()
          await authTokenStore.delete()
          return { success: true as const }
        },
      },
    }
  },
})

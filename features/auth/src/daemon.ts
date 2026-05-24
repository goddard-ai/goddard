import { definePlugin } from "@goddard-ai/daemon-plugin"

import { authIpcRoutes } from "./daemon-ipc.ts"

export const authPlugin = definePlugin({
  name: "auth",
  ipcRoutes: authIpcRoutes,
  setup({ authBackendClient, authTokenStore }) {
    return {
      routeHandlers: {
        auth: {
          device: {
            start: async ({ body }) => authBackendClient.startDeviceFlow(body),
            complete: async ({ body }) => {
              const session = await authBackendClient.completeDeviceFlow(body)
              await authTokenStore.set(session.token)
              return session
            },
          },
          whoami: async () => authBackendClient.whoami(),
          logout: async () => {
            await authBackendClient.logout()
            await authTokenStore.delete()
            return { success: true as const }
          },
        },
      },
    }
  },
})

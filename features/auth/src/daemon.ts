import { definePlugin } from "@goddard-ai/daemon-plugin"

import { authBackendRoutes } from "./backend.ts"
import { authIpcRoutes } from "./daemon-ipc.ts"

export const authPlugin = definePlugin({
  name: "auth",
  backendRoutes: authBackendRoutes,
  ipcRoutes: authIpcRoutes,
  setup({ authTokenStore, backend }) {
    return {
      ipcHandlers: {
        auth: {
          device: {
            start: async ({ body }) => backend.auth.device.start({ body }),
            complete: async ({ body }) => {
              const session = await backend.auth.device.complete({ body })
              await authTokenStore.set(session.token)
              return session
            },
          },
          whoami: async () => backend.auth.session.current(),
          logout: async () => {
            await authTokenStore.delete()
            return { success: true as const }
          },
        },
      },
    }
  },
})

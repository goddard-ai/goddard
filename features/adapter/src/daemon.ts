import { definePlugin } from "@goddard-ai/daemon-plugin"

import { adapterIpcRoutes } from "./daemon-ipc.ts"
import { listAdapters } from "./list-adapters.ts"

export const adapterPlugin = definePlugin({
  name: "adapter",
  ipcRoutes: adapterIpcRoutes,
  setup(context) {
    return {
      ipcHandlers: {
        adapter: {
          list: async ({ body }) => listAdapters(context, body),
        },
      },
    }
  },
})

import { definePlugin } from "@goddard-ai/daemon-plugin"

import { adapterIpcRoutes } from "./daemon-ipc.ts"
import { installCatalogAdapter, listAdapters, uninstallCatalogAdapter } from "./list-adapters.ts"

export const adapterPlugin = definePlugin({
  name: "adapter",
  ipcRoutes: adapterIpcRoutes,
  setup(context) {
    return {
      ipcHandlers: {
        adapter: {
          list: async ({ body }) => listAdapters(context, body),
          install: async ({ body }) => installCatalogAdapter(context, body),
          uninstall: async ({ body }) => uninstallCatalogAdapter(context, body),
        },
      },
    }
  },
})

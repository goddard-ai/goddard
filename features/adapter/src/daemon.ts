import { definePlugin } from "@goddard-ai/daemon-plugin"

import { adapterIpcRoutes } from "./daemon-ipc.ts"
import { installCatalogAdapter, listAdapters, uninstallCatalogAdapter } from "./list-adapters.ts"

export const adapterPlugin = definePlugin({
  name: "adapter",
  ipcRoutes: adapterIpcRoutes,
  setup({ agentInstallService, configProvider, registryService }) {
    const adapterContext = {
      agentInstallService,
      configProvider,
      registryService,
    }

    return {
      ipcHandlers: {
        adapter: {
          list: async ({ body }) => listAdapters(adapterContext, body),
          install: async ({ body }) => installCatalogAdapter(adapterContext, body),
          uninstall: async ({ body }) => uninstallCatalogAdapter(adapterContext, body),
        },
      },
    }
  },
})

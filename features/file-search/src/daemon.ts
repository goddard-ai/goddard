import { definePlugin } from "@goddard-ai/daemon-plugin"

import { fileSearchIpcRoutes } from "./daemon-ipc.ts"
import { destroyFileSearchManager, getComposerEntries } from "./daemon/composer-entries.ts"

export const fileSearchPlugin = definePlugin({
  name: "file-search",
  ipcRoutes: fileSearchIpcRoutes,
  setup() {
    return {
      close: destroyFileSearchManager,
      ipcHandlers: {
        fileSearch: {
          composerEntries: async ({ body }) => getComposerEntries(body),
        },
      },
    }
  },
})

import { definePlugin } from "@goddard-ai/daemon-plugin"

import { fileSearchIpcRoutes } from "./daemon-ipc.ts"
import { getComposerEntries } from "./daemon/composer-entries.ts"

export const fileSearchPlugin = definePlugin({
  name: "file-search",
  ipcRoutes: fileSearchIpcRoutes,
  setup() {
    return {
      ipcHandlers: {
        fileSearch: {
          composerEntries: async ({ body }) => getComposerEntries(body),
        },
      },
    }
  },
})

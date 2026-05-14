import { definePlugin } from "@goddard-ai/daemon-plugin"

import { adapterIpcSchema } from "./daemon-ipc.ts"
import { listAdapters } from "./list-adapters.ts"

export const adapterPlugin = definePlugin({
  name: "adapter",
  ipc: adapterIpcSchema,
  setup(context) {
    return {
      requestHandlers: {
        "adapter.list": async (input) => listAdapters(context, input),
      },
    }
  },
})

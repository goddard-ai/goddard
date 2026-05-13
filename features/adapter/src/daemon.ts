import { definePlugin, defineSetupContext } from "@goddard-ai/daemon-plugin"

import { adapterIpcSchema } from "./daemon-ipc.ts"
import { listAdapters, type ListAdaptersContext } from "./list-adapters.ts"

export const adapterPlugin = definePlugin({
  name: "adapter",
  ipc: adapterIpcSchema,
  setupContext: defineSetupContext<ListAdaptersContext>(),
  setup(context) {
    return {
      requestHandlers: {
        "adapter.list": async (input) => listAdapters(context, input),
      },
    }
  },
})

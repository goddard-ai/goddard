import { definePlugin } from "@goddard-ai/daemon-plugin"

import { adapterIpcSchema } from "./daemon-ipc.ts"
import { listAdapters, type ListAdaptersContext } from "./list-adapters.ts"
import type { ListAdaptersRequestType } from "./schema.ts"

export const adapterPlugin = definePlugin({
  name: "adapter",
  ipc: adapterIpcSchema,
  createRequestHandlers(context: ListAdaptersContext) {
    return {
      "adapter.list": async (input: ListAdaptersRequestType) => listAdapters(context, input),
    }
  },
})

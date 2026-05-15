import { defineRequest, defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { adapterIpcSchema } from "./daemon-ipc.ts"
import type { ListAdaptersRequestType } from "./schema.ts"

export const adapterSdkPlugin = defineSdkPlugin({
  name: "adapter",
  ipc: adapterIpcSchema,
  create({ client }) {
    const listAdapters = defineRequest(client, "adapter.list")

    return {
      adapter: {
        /** Lists adapters available for one project or global launch flow. */
        list: (input: ListAdaptersRequestType = {}) => listAdapters(input),
      },
    }
  },
})

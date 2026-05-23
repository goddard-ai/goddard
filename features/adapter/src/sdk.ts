import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { adapterIpcRoutes } from "./daemon-ipc.ts"
import type { ListAdaptersRequestType } from "./schema.ts"

export const adapterSdkPlugin = defineSdkPlugin({
  name: "adapter",
  ipcRoutes: adapterIpcRoutes,
  extend({ client }) {
    return {
      adapter: {
        /** Lists adapters available for one project or global launch flow. */
        list: (input: ListAdaptersRequestType) => client.send("adapter.list", input),
      },
    }
  },
})

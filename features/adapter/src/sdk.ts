import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import type { ListAdaptersRequestType } from "./schema.ts"

/** Builds the adapter namespace with one thin method per daemon adapter IPC action. */
export function createAdapterNamespace(client: DaemonIpcClient) {
  return {
    /** Lists adapters available for one project or global launch flow. */
    list: async (input: ListAdaptersRequestType = {}) => client.send("adapter.list", input),
  }
}

export const adapterSdkPlugin = defineSdkPlugin({
  name: "adapter",
  create({ client }: { client: DaemonIpcClient }) {
    return {
      adapter: createAdapterNamespace(client),
    }
  },
})

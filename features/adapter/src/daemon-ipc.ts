import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import { ListAdaptersRequest, type ListAdaptersResponse } from "./schema.ts"

export const adapterIpcRoutes = defineIpcRoutes({
  adapter: http.resource("adapter", {
    /** Lists adapters available for one project or global launch flow. */
    list: http.post("list", {
      body: ListAdaptersRequest,
      response: $type<ListAdaptersResponse>(),
    }),
  }),
})

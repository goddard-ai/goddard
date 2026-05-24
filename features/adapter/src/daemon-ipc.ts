import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import { ListAdaptersRequest, type ListAdaptersResponse } from "./schema.ts"

export const adapterIpcRoutes = defineIpcRoutes({
  adapter: http.resource("adapter", {
    list: http.post("list", {
      body: ListAdaptersRequest,
      response: $type<ListAdaptersResponse>(),
    }),
  }),
})

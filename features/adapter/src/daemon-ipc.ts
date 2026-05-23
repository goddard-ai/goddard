import { $type, defineIpcRoutes, defineIpcSchema, http } from "@goddard-ai/ipc"

import { ListAdaptersRequest, type ListAdaptersResponse } from "./schema.ts"

export const adapterIpcSchema = defineIpcSchema({
  requests: {
    "adapter.list": {
      payload: ListAdaptersRequest,
      response: $type<ListAdaptersResponse>(),
    },
  },
  streams: {},
})

export const adapterIpcRoutes = defineIpcRoutes({
  adapter: http.resource("adapter", {
    list: http.post("list", {
      body: ListAdaptersRequest,
      response: $type<ListAdaptersResponse>(),
    }),
  }),
})

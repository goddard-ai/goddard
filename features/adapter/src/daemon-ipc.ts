import { $type, defineIpcSchema } from "@goddard-ai/ipc"

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

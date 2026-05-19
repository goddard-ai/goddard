import { $type, defineIpcSchema } from "@goddard-ai/ipc"

import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./schema.ts"

export const authIpcSchema = defineIpcSchema({
  requests: {
    "auth.device.start": {
      payload: DeviceFlowStart,
      response: $type<DeviceFlowSession>(),
    },
    "auth.device.complete": {
      payload: DeviceFlowComplete,
      response: $type<AuthSession>(),
    },
    "auth.whoami": {
      response: $type<AuthSession>(),
    },
    "auth.logout": {
      response: $type<{ success: true }>(),
    },
  },
  streams: {},
})

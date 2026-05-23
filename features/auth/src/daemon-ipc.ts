import { $type, defineIpcRoutes, defineIpcSchema, http } from "@goddard-ai/ipc"

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

export const authIpcRoutes = defineIpcRoutes({
  auth: http.resource("auth", {
    device: http.resource("device", {
      start: http.post("start", {
        body: DeviceFlowStart,
        response: $type<DeviceFlowSession>(),
      }),
      complete: http.post("complete", {
        body: DeviceFlowComplete,
        response: $type<AuthSession>(),
      }),
    }),
    whoami: http.get("whoami", {
      response: $type<AuthSession>(),
    }),
    logout: http.post("logout", {
      response: $type<{ success: true }>(),
    }),
  }),
})

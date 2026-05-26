import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./schema.ts"

export const authIpcRoutes = defineIpcRoutes({
  auth: http.resource("auth", {
    device: http.resource("device", {
      /** Starts one GitHub device flow through the daemon auth contract. */
      start: http.post("start", {
        body: DeviceFlowStart,
        response: $type<DeviceFlowSession>(),
      }),
      /** Completes one pending GitHub device flow through the daemon auth contract. */
      complete: http.post("complete", {
        body: DeviceFlowComplete,
        response: $type<AuthSession>(),
      }),
    }),
    /** Reads the current daemon-owned auth session as-is. */
    whoami: http.get("whoami", {
      response: $type<AuthSession>(),
    }),
    /** Clears the current daemon-owned auth session as-is. */
    logout: http.post("logout", {
      response: $type<{ success: true }>(),
    }),
  }),
})

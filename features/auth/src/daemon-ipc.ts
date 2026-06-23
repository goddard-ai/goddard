import { $type, defineIpcRoutes, http, metadata } from "@goddard-ai/ipc"

import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./schema.ts"

export const authIpcRoutes = defineIpcRoutes({
  auth: http.resource("auth", {
    ...metadata({
      description: "Authentication session control.",
    }),
    device: http.resource("device", {
      ...metadata({
        description: "GitHub device-flow authentication control.",
      }),
      /** Starts one GitHub device flow through the auth contract. */
      start: http.post("start", {
        ...metadata({
          description: "Starts one GitHub device flow through the auth contract.",
        }),
        body: DeviceFlowStart,
        response: $type<DeviceFlowSession>(),
      }),
      /** Completes one pending GitHub device flow through the auth contract. */
      complete: http.post("complete", {
        ...metadata({
          description: "Completes one pending GitHub device flow through the auth contract.",
        }),
        body: DeviceFlowComplete,
        response: $type<AuthSession>(),
      }),
    }),
    /** Reads the current auth session as-is. */
    whoami: http.get("whoami", {
      ...metadata({
        description: "Reads the current auth session as-is.",
      }),
      response: $type<AuthSession>(),
    }),
    /** Clears the current auth session as-is. */
    logout: http.post("logout", {
      ...metadata({
        description: "Clears the current auth session as-is.",
      }),
      response: $type<{ success: true }>(),
    }),
  }),
})

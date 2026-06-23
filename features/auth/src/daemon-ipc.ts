import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"

import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./schema.ts"

export const authIpcRoutes = defineIpcRoutes({
  auth: http.resource("auth", {
    ...ipcMetadata({
      description: "Authentication session control.",
    }),
    device: http.resource("device", {
      ...ipcMetadata({
        description: "GitHub device-flow authentication control.",
      }),
      /** Starts one GitHub device flow through the auth contract. */
      start: http.post("start", {
        ...ipcMetadata({
          description: "Starts one GitHub device flow through the auth contract.",
        }),
        body: DeviceFlowStart,
        response: $type<DeviceFlowSession>(),
      }),
      /** Completes one pending GitHub device flow through the auth contract. */
      complete: http.post("complete", {
        ...ipcMetadata({
          description: "Completes one pending GitHub device flow through the auth contract.",
        }),
        body: DeviceFlowComplete,
        response: $type<AuthSession>(),
      }),
    }),
    /** Reads the current auth session as-is. */
    whoami: http.get("whoami", {
      ...ipcMetadata({
        description: "Reads the current auth session as-is.",
      }),
      response: $type<AuthSession>(),
    }),
    /** Clears the current auth session as-is. */
    logout: http.post("logout", {
      ...ipcMetadata({
        description: "Clears the current auth session as-is.",
      }),
      response: $type<{ success: true }>(),
    }),
  }),
})

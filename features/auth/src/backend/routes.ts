import { $type, defineBackendRoutes, http, metadata } from "@goddard-ai/backend-plugin"

import {
  AuthSession,
  BearerHeaders,
  DeviceFlowComplete,
  DeviceFlowStart,
  type DeviceFlowSession,
} from "../schema.ts"

/** Auth-owned backend routes grouped by the authenticated session workflow. */
export const authBackendRoutes = defineBackendRoutes({
  auth: http.resource("auth", {
    ...metadata({
      description: "Backend authentication workflow routes.",
    }),
    device: http.resource("device", {
      ...metadata({
        description: "Backend device-flow authentication control.",
      }),
      start: http.post("start", {
        ...metadata({
          description: "Starts one backend device-flow authentication session.",
        }),
        body: DeviceFlowStart,
        response: $type<DeviceFlowSession>(),
      }),
      complete: http.post("complete", {
        ...metadata({
          description: "Completes one backend device-flow authentication session.",
        }),
        body: DeviceFlowComplete,
        response: $type<AuthSession>(),
      }),
    }),
    session: http.resource("session", {
      ...metadata({
        description: "Backend authenticated session lookup.",
      }),
      current: http.get("current", {
        ...metadata({
          description: "Reads the current backend authenticated session.",
        }),
        headers: BearerHeaders,
        response: $type<AuthSession>(),
      }),
    }),
  }),
})

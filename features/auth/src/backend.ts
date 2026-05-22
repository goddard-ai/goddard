import { $type } from "rouzer"
import * as http from "rouzer/http"

import {
  AuthSession,
  BearerHeaders,
  DeviceFlowComplete,
  DeviceFlowStart,
  type DeviceFlowSession,
} from "./schema.ts"

/** Auth-owned backend routes grouped by the authenticated session workflow. */
export const auth = http.resource("auth", {
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
  session: http.resource("session", {
    current: http.get("current", {
      headers: BearerHeaders,
      response: $type<AuthSession>(),
    }),
  }),
})

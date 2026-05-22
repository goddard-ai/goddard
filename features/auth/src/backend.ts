import { $type } from "rouzer"
import * as http from "rouzer/http"

import {
  AuthSession,
  BearerHeaders,
  DeviceFlowComplete,
  DeviceFlowStart,
  type DeviceFlowSession,
} from "./schema.ts"

/** Starts the GitHub device flow for a pending user session. */
export const authDeviceStart = http.post("auth/device/start", {
  body: DeviceFlowStart,
  response: $type<DeviceFlowSession>(),
})

/** Completes the GitHub device flow and returns an authenticated backend session. */
export const authDeviceComplete = http.post("auth/device/complete", {
  body: DeviceFlowComplete,
  response: $type<AuthSession>(),
})

/** Reads the current authenticated backend session. */
export const authSession = http.get("auth/session", {
  headers: BearerHeaders,
  response: $type<AuthSession>(),
})

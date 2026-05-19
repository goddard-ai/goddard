import { $type, route } from "rouzer"

import {
  AuthSession,
  BearerHeaders,
  DeviceFlowComplete,
  DeviceFlowStart,
  type DeviceFlowSession,
} from "./schema.ts"

/** Starts the GitHub device flow for a pending user session. */
export const authDeviceStartRoute = route("auth/device/start", {
  POST: {
    body: DeviceFlowStart,
    response: $type<DeviceFlowSession>(),
  },
})

/** Completes the GitHub device flow and returns an authenticated backend session. */
export const authDeviceCompleteRoute = route("auth/device/complete", {
  POST: {
    body: DeviceFlowComplete,
    response: $type<AuthSession>(),
  },
})

/** Reads the current authenticated backend session. */
export const authSessionRoute = route("auth/session", {
  GET: {
    headers: BearerHeaders,
    response: $type<AuthSession>(),
  },
})

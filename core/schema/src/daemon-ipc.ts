import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"
import { z } from "zod"

/** Core daemon IPC error codes shared by clients and non-feature daemon surfaces. */
export const DaemonIpcErrorCodes = {
  InvalidRequest: "daemon.invalid_request",
  Unavailable: "daemon.unavailable",
} as const

export type DaemonIpcErrorCode = (typeof DaemonIpcErrorCodes)[keyof typeof DaemonIpcErrorCodes]

export const DaemonIpcErrorCode = z.enum([
  DaemonIpcErrorCodes.InvalidRequest,
  DaemonIpcErrorCodes.Unavailable,
])

/** Core daemon IPC routes that are not owned by feature packages. */
export const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
  }),
})

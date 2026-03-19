import { $type, route } from "rouzer"
import type {
  CreateDaemonSessionResponse,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  ShutdownDaemonSessionResponse,
} from "../../daemon.js"
import { CreateDaemonSessionRequest, DaemonSessionPathParams } from "../sessions.js"

/** Creates one daemon-managed session. */
export const sessionCreateRoute = route("sessions", {
  POST: {
    body: CreateDaemonSessionRequest,
    response: $type<CreateDaemonSessionResponse>(),
  },
})

/** Fetches the current state for one daemon-managed session. */
export const sessionGetRoute = route("sessions/:id", {
  GET: {
    path: DaemonSessionPathParams,
    response: $type<GetDaemonSessionResponse>(),
  },
})

/** Fetches the persisted history for one daemon-managed session. */
export const sessionHistoryRoute = route("sessions/:id/history", {
  GET: {
    path: DaemonSessionPathParams,
    response: $type<GetDaemonSessionHistoryResponse>(),
  },
})

/** Shuts down one daemon-managed session. */
export const sessionShutdownRoute = route("sessions/:id/shutdown", {
  POST: {
    path: DaemonSessionPathParams,
    response: $type<ShutdownDaemonSessionResponse>(),
  },
})

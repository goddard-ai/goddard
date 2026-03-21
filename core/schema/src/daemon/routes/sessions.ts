import { $type, route } from "rouzer"
import type {
  CreateDaemonSessionResponse,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  ListDaemonSessionsResponse,
  ShutdownDaemonSessionResponse,
} from "../../daemon.js"
import {
  CreateDaemonSessionRequest,
  DaemonSessionPathParams,
  ListDaemonSessionsRequest,
} from "../sessions.js"

/** Fetches one page of daemon-managed sessions in stable recency order. */
export const sessionListRoute = route("sessions", {
  GET: {
    query: ListDaemonSessionsRequest,
    response: $type<ListDaemonSessionsResponse>(),
  },
})

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

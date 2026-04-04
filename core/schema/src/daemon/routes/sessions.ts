import { $type, route } from "rouzer"
import type {
  CreateDaemonSessionResponse,
  GetDaemonSessionDiagnosticsResponse,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  GetDaemonSessionWorkforceResponse,
  GetDaemonSessionWorktreeResponse,
  ListDaemonSessionsResponse,
  ShutdownDaemonSessionResponse,
} from "../sessions.ts"
import {
  CreateDaemonSessionRequest,
  DaemonSessionPathParams,
  ListDaemonSessionsRequest,
} from "../sessions.ts"

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

/** Fetches persisted diagnostic events for one daemon-managed session. */
export const sessionDiagnosticsRoute = route("sessions/:id/diagnostics", {
  GET: {
    path: DaemonSessionPathParams,
    response: $type<GetDaemonSessionDiagnosticsResponse>(),
  },
})

/** Fetches persisted worktree metadata for one daemon-managed session. */
export const sessionWorktreeRoute = route("sessions/:id/worktree", {
  GET: {
    path: DaemonSessionPathParams,
    response: $type<GetDaemonSessionWorktreeResponse>(),
  },
})

/** Fetches persisted workforce metadata for one daemon-managed session. */
export const sessionWorkforceRoute = route("sessions/:id/workforce", {
  GET: {
    path: DaemonSessionPathParams,
    response: $type<GetDaemonSessionWorkforceResponse>(),
  },
})

/** Shuts down one daemon-managed session. */
export const sessionShutdownRoute = route("sessions/:id/shutdown", {
  POST: {
    path: DaemonSessionPathParams,
    response: $type<ShutdownDaemonSessionResponse>(),
  },
})

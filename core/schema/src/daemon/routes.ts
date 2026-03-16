import { $type, route } from "rouzer"
import * as z from "zod/mini"
import type {
  CreateSessionRequest,
  CreateSessionResponse,
  DaemonHealth,
  ReplyPrDaemonResponse,
  Session,
  SessionHistory,
  SubmitPrDaemonResponse,
} from "../daemon.ts"

const bearerHeaderSchema = z.object({
  authorization: z.string(),
})

export const healthRoute = route("health", {
  GET: {
    response: $type<DaemonHealth>(),
  },
})

export const prSubmitRoute = route("pr/submit", {
  POST: {
    headers: bearerHeaderSchema,
    body: z.object({
      cwd: z.string(),
      title: z.string(),
      body: z.string(),
      head: z.optional(z.string()),
      base: z.optional(z.string()),
    }),
    response: $type<SubmitPrDaemonResponse>(),
  },
})

export const prReplyRoute = route("pr/reply", {
  POST: {
    headers: bearerHeaderSchema,
    body: z.object({
      cwd: z.string(),
      message: z.string(),
      prNumber: z.optional(z.number()),
    }),
    response: $type<ReplyPrDaemonResponse>(),
  },
})

export const sessionsCreateRoute = route("sessions", {
  POST: {
    headers: bearerHeaderSchema,
    body: $type<CreateSessionRequest>(),
    response: $type<CreateSessionResponse>(),
  },
})

export const sessionsGetRoute = route("sessions/:id", {
  GET: {
    headers: bearerHeaderSchema,
    response: $type<Session>(),
  },
})

export const sessionsHistoryRoute = route("sessions/:id/history", {
  GET: {
    headers: bearerHeaderSchema,
    response: $type<SessionHistory>(),
  },
})

export const sessionsShutdownRoute = route("sessions/:id/shutdown", {
  POST: {
    headers: bearerHeaderSchema,
    response: $type<{ success: boolean }>(),
  },
})

export const sessionsAcpWsRoute = route("sessions/:id/acp", {
  WS: {
    headers: bearerHeaderSchema,
  },
})

export type {
  CreateSessionRequest,
  CreateSessionResponse,
  ReplyPrDaemonRequest,
  Session,
  SessionHistory,
  SubmitPrDaemonRequest,
} from "../daemon.ts"

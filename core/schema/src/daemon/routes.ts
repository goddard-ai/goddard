import { $type, route } from "rouzer"
import * as z from "zod/mini"
import type {
  CreateDaemonSessionResponse,
  DaemonHealth,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  ReplyPrDaemonResponse,
  ShutdownDaemonSessionResponse,
  SubmitPrDaemonResponse,
} from "../daemon.ts"

export const healthRoute = route("health", {
  GET: {
    response: $type<DaemonHealth>(),
  },
})

export const prSubmitRoute = route("pr/submit", {
  POST: {
    headers: z.object({
      authorization: z.string(),
    }),
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
    headers: z.object({
      authorization: z.string(),
    }),
    body: z.object({
      cwd: z.string(),
      message: z.string(),
      prNumber: z.optional(z.number()),
    }),
    response: $type<ReplyPrDaemonResponse>(),
  },
})

export const sessionCreateRoute = route("sessions", {
  POST: {
    body: z.object({
      agent: z.union([
        z.string(),
        z.object({
          type: z.enum(["binary", "npx", "uvx"]),
          package: z.optional(z.string()),
          cmd: z.optional(z.string()),
          args: z.optional(z.array(z.string())),
        }),
      ]),
      cwd: z.string(),
      mcpServers: z.array(z.unknown()),
      systemPrompt: z.string(),
      env: z.optional(z.unknown()),
      metadata: z.optional(z.unknown()),
      initialPrompt: z.optional(z.union([z.string(), z.array(z.unknown())])),
      oneShot: z.optional(z.boolean()),
    }),
    response: $type<CreateDaemonSessionResponse>(),
  },
})

export const sessionGetRoute = route("sessions/:id", {
  GET: {
    path: z.object({
      id: z.string(),
    }),
    response: $type<GetDaemonSessionResponse>(),
  },
})

export const sessionHistoryRoute = route("sessions/:id/history", {
  GET: {
    path: z.object({
      id: z.string(),
    }),
    response: $type<GetDaemonSessionHistoryResponse>(),
  },
})

export const sessionShutdownRoute = route("sessions/:id/shutdown", {
  POST: {
    path: z.object({
      id: z.string(),
    }),
    response: $type<ShutdownDaemonSessionResponse>(),
  },
})

/**
 * ACP transport attaches over websocket upgrades on this path.
 * Rouzer captures the path contract while runtime handles `Upgrade: websocket`.
 */
export const sessionAcpWebSocketRoute = route("sessions/:id/acp", {
  GET: {
    path: z.object({
      id: z.string(),
    }),
  },
})

export type {
  CreateDaemonSessionRequest,
  DaemonSessionPathParams,
  ReplyPrDaemonRequest,
  SubmitPrDaemonRequest,
} from "../daemon.ts"

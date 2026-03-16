import { $type, route } from "rouzer"
import * as z from "zod/mini"
import type { DaemonHealth, ReplyPrDaemonResponse, SubmitPrDaemonResponse } from "../daemon.ts"

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

export type { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "../daemon.ts"

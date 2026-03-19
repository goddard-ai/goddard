import { $type, route } from "rouzer"
import * as z from "zod/mini"
import { agentBinaryPlatforms } from "../session-server.js"
import type {
  CreateDaemonSessionResponse,
  DaemonHealth,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  ReplyPrDaemonResponse,
  ShutdownDaemonSessionResponse,
  SubmitPrDaemonResponse,
} from "../daemon.js"

const agentBinaryTargetRouteSchema = z.strictObject({
  archive: z.url(),
  cmd: z.string(),
  args: z.optional(z.array(z.string())),
  env: z.optional(z.record(z.string(), z.string())),
})

const agentBinaryDistributionRouteSchema = z.strictObject(
  Object.fromEntries(
    agentBinaryPlatforms.map((platform) => [platform, z.optional(agentBinaryTargetRouteSchema)]),
  ) as Record<
    (typeof agentBinaryPlatforms)[number],
    ReturnType<typeof z.optional<typeof agentBinaryTargetRouteSchema>>
  >,
)

const agentPackageDistributionRouteSchema = z.strictObject({
  package: z.string().check(z.minLength(1)),
  args: z.optional(z.array(z.string())),
  env: z.optional(z.record(z.string(), z.string())),
})

const agentDistributionRouteSchema = z.strictObject({
  id: z.string().check(z.regex(/^[a-z][a-z0-9-]*$/)),
  name: z.string().check(z.minLength(1)),
  version: z.string().check(z.regex(/^[0-9]+\.[0-9]+\.[0-9]+/)),
  description: z.string().check(z.minLength(1)),
  repository: z.optional(z.url()),
  authors: z.optional(z.array(z.string())),
  license: z.optional(z.string()),
  icon: z.optional(z.string()),
  distribution: z.strictObject({
    binary: z.optional(agentBinaryDistributionRouteSchema),
    npx: z.optional(agentPackageDistributionRouteSchema),
    uvx: z.optional(agentPackageDistributionRouteSchema),
  }),
})

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
      agent: z.union([z.string(), agentDistributionRouteSchema]),
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

export type {
  CreateDaemonSessionRequest,
  DaemonSessionPathParams,
  ReplyPrDaemonRequest,
  SubmitPrDaemonRequest,
} from "../daemon.js"

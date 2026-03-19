import { $type } from "@goddard-ai/ipc"
import { z } from "zod"
import { agentBinaryPlatforms } from "./session-server.js"
import type {
  CreateDaemonSessionResponse,
  DaemonHealth,
  GetDaemonWorkforceResponse,
  GetDaemonSessionDiagnosticsResponse,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  ListDaemonWorkforcesResponse,
  MutateDaemonWorkforceResponse,
  ReplyPrDaemonResponse,
  ShutdownDaemonWorkforceResponse,
  ShutdownDaemonSessionResponse,
  StartDaemonWorkforceResponse,
  SubmitPrDaemonResponse,
} from "./daemon.js"

const agentBinaryTargetSchema = z
  .object({
    archive: z.string().url(),
    cmd: z.string(),
    args: z.array(z.string()).optional(),
    env: z.record(z.string(), z.string()).optional(),
  })
  .strict()

const agentBinaryDistributionSchema = z
  .object(
    Object.fromEntries(
      agentBinaryPlatforms.map((platform) => [platform, agentBinaryTargetSchema.optional()]),
    ) as Record<
      (typeof agentBinaryPlatforms)[number],
      z.ZodOptional<typeof agentBinaryTargetSchema>
    >,
  )
  .strict()
  .refine((value) => Object.values(value).some((target) => target !== undefined), {
    message: "binary distributions must declare at least one supported platform",
  })

const agentPackageDistributionSchema = z
  .object({
    package: z.string().min(1),
    args: z.array(z.string()).optional(),
    env: z.record(z.string(), z.string()).optional(),
  })
  .strict()

const agentDistributionSchema = z
  .object({
    id: z.string().regex(/^[a-z][a-z0-9-]*$/),
    name: z.string().min(1),
    version: z.string().regex(/^[0-9]+\.[0-9]+\.[0-9]+/),
    description: z.string().min(1),
    repository: z.string().url().optional(),
    authors: z.array(z.string()).optional(),
    license: z.string().optional(),
    icon: z.string().optional(),
    distribution: z
      .object({
        binary: agentBinaryDistributionSchema.optional(),
        npx: agentPackageDistributionSchema.optional(),
        uvx: agentPackageDistributionSchema.optional(),
      })
      .strict()
      .refine((value) => Object.values(value).some((entry) => entry !== undefined), {
        message: "distribution must declare at least one install method",
      }),
  })
  .strict()

const daemonSessionMetadataSchema = z
  .object({
    repository: z.string().optional(),
    prNumber: z.number().int().optional(),
  })
  .catchall(z.unknown())

const workforceTokenSchema = z.string().optional()
const workforceRequestIntentSchema = z.enum(["default", "create"]).optional()

export const daemonIpcSchema = {
  client: {
    requests: {
      health: {
        payload: z.object({}),
        response: $type<DaemonHealth>(),
      },
      prSubmit: {
        payload: z.object({
          token: z.string(),
          cwd: z.string(),
          title: z.string(),
          body: z.string(),
          head: z.string().optional(),
          base: z.string().optional(),
        }),
        response: $type<SubmitPrDaemonResponse>(),
      },
      prReply: {
        payload: z.object({
          token: z.string(),
          cwd: z.string(),
          message: z.string(),
          prNumber: z.number().int().optional(),
        }),
        response: $type<ReplyPrDaemonResponse>(),
      },
      sessionCreate: {
        payload: z.object({
          agent: z.union([z.string(), agentDistributionSchema]),
          cwd: z.string(),
          mcpServers: z.array(z.unknown()),
          systemPrompt: z.string(),
          env: z.record(z.string(), z.string()).optional(),
          metadata: daemonSessionMetadataSchema.optional(),
          initialPrompt: z.union([z.string(), z.array(z.unknown())]).optional(),
          oneShot: z.boolean().optional(),
        }),
        response: $type<CreateDaemonSessionResponse>(),
      },
      sessionGet: {
        payload: z.object({ id: z.string() }),
        response: $type<GetDaemonSessionResponse>(),
      },
      sessionConnect: {
        payload: z.object({ id: z.string() }),
        response: $type<GetDaemonSessionResponse>(),
      },
      sessionHistory: {
        payload: z.object({ id: z.string() }),
        response: $type<GetDaemonSessionHistoryResponse>(),
      },
      sessionDiagnostics: {
        payload: z.object({ id: z.string() }),
        response: $type<GetDaemonSessionDiagnosticsResponse>(),
      },
      sessionShutdown: {
        payload: z.object({ id: z.string() }),
        response: $type<ShutdownDaemonSessionResponse>(),
      },
      sessionSend: {
        payload: z.object({
          id: z.string(),
          message: z.unknown(),
        }),
        response: $type<{ accepted: true }>(),
      },
      sessionResolveToken: {
        payload: z.object({ token: z.string() }),
        response: $type<{ id: string }>(),
      },
      workforceStart: {
        payload: z.object({
          rootDir: z.string(),
        }),
        response: $type<StartDaemonWorkforceResponse>(),
      },
      workforceGet: {
        payload: z.object({
          rootDir: z.string(),
        }),
        response: $type<GetDaemonWorkforceResponse>(),
      },
      workforceList: {
        payload: z.object({}),
        response: $type<ListDaemonWorkforcesResponse>(),
      },
      workforceShutdown: {
        payload: z.object({
          rootDir: z.string(),
        }),
        response: $type<ShutdownDaemonWorkforceResponse>(),
      },
      workforceRequest: {
        payload: z.object({
          rootDir: z.string(),
          targetAgentId: z.string(),
          input: z.string(),
          intent: workforceRequestIntentSchema,
          token: workforceTokenSchema,
        }),
        response: $type<MutateDaemonWorkforceResponse>(),
      },
      workforceUpdate: {
        payload: z.object({
          rootDir: z.string(),
          requestId: z.string(),
          input: z.string(),
          token: workforceTokenSchema,
        }),
        response: $type<MutateDaemonWorkforceResponse>(),
      },
      workforceCancel: {
        payload: z.object({
          rootDir: z.string(),
          requestId: z.string(),
          reason: z.string().optional(),
          token: workforceTokenSchema,
        }),
        response: $type<MutateDaemonWorkforceResponse>(),
      },
      workforceTruncate: {
        payload: z.object({
          rootDir: z.string(),
          agentId: z.string().optional(),
          reason: z.string().optional(),
          token: workforceTokenSchema,
        }),
        response: $type<MutateDaemonWorkforceResponse>(),
      },
      workforceRespond: {
        payload: z.object({
          rootDir: z.string(),
          requestId: z.string(),
          output: z.string(),
          token: z.string(),
        }),
        response: $type<MutateDaemonWorkforceResponse>(),
      },
      workforceSuspend: {
        payload: z.object({
          rootDir: z.string(),
          requestId: z.string(),
          reason: z.string(),
          token: z.string(),
        }),
        response: $type<MutateDaemonWorkforceResponse>(),
      },
    },
  },
  server: {
    streams: {
      sessionMessage: z.object({
        id: z.string(),
        message: z.unknown(),
      }),
    },
  },
}

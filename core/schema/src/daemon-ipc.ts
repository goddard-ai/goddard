import { $type } from "@goddard-ai/ipc"
import { z } from "zod"
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

const agentDistributionSchema = z.object({
  type: z.enum(["binary", "npx", "uvx"]),
  package: z.string().optional(),
  cmd: z.string().optional(),
  args: z.array(z.string()).optional(),
})

const daemonSessionMetadataSchema = z
  .object({
    repository: z.string().optional(),
    prNumber: z.number().int().optional(),
  })
  .catchall(z.unknown())

const workforceTokenSchema = z.string().optional()

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

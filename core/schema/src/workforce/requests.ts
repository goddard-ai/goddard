import { z } from "zod"

/** Stable request intents supported by workforce mutation APIs. */
export const WorkforceRequestIntent = z.enum(["default", "create"])

export type WorkforceRequestIntent = z.infer<typeof WorkforceRequestIntent>

/** Optional daemon-issued workforce continuation token. */
export const WorkforceToken = z.string().optional()

export type WorkforceToken = z.infer<typeof WorkforceToken>

/** Request payload used to start one daemon-owned workforce runtime. */
export const StartDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
})

export type StartDaemonWorkforceRequest = z.infer<typeof StartDaemonWorkforceRequest>

/** Request payload used to fetch one daemon-owned workforce runtime. */
export const GetDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
})

export type GetDaemonWorkforceRequest = z.infer<typeof GetDaemonWorkforceRequest>

/** Request payload used to stop one daemon-owned workforce runtime. */
export const ShutdownDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
})

export type ShutdownDaemonWorkforceRequest = z.infer<typeof ShutdownDaemonWorkforceRequest>

/** Request payload used to enqueue work for one target workforce agent. */
export const CreateDaemonWorkforceRequestRequest = z.object({
  rootDir: z.string(),
  targetAgentId: z.string(),
  input: z.string(),
  intent: WorkforceRequestIntent.optional(),
  token: WorkforceToken,
})

export type CreateDaemonWorkforceRequestRequest = z.infer<
  typeof CreateDaemonWorkforceRequestRequest
>

/** Request payload used to add resume context to one workforce request. */
export const UpdateDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
  requestId: z.string(),
  input: z.string(),
  token: WorkforceToken,
})

export type UpdateDaemonWorkforceRequest = z.infer<typeof UpdateDaemonWorkforceRequest>

/** Request payload used to cancel one workforce request. */
export const CancelDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
  requestId: z.string(),
  reason: z.string().optional(),
  token: WorkforceToken,
})

export type CancelDaemonWorkforceRequest = z.infer<typeof CancelDaemonWorkforceRequest>

/** Request payload used to clear pending work in one agent scope or the whole runtime. */
export const TruncateDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
  agentId: z.string().optional(),
  reason: z.string().optional(),
  token: WorkforceToken,
})

export type TruncateDaemonWorkforceRequest = z.infer<typeof TruncateDaemonWorkforceRequest>

/** Request payload used by an active workforce agent to finish its current task. */
export const RespondDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
  requestId: z.string(),
  output: z.string(),
  token: z.string(),
})

export type RespondDaemonWorkforceRequest = z.infer<typeof RespondDaemonWorkforceRequest>

/** Request payload used by an active workforce agent to suspend its current task. */
export const SuspendDaemonWorkforceRequest = z.object({
  rootDir: z.string(),
  requestId: z.string(),
  reason: z.string(),
  token: z.string(),
})

export type SuspendDaemonWorkforceRequest = z.infer<typeof SuspendDaemonWorkforceRequest>

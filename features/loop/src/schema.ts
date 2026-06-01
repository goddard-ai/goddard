import { mergeConfigLayers, selectLast } from "@goddard-ai/config"
import { AgentSetting, McpServer, StaticSessionParams } from "@goddard-ai/schema/config"
import { CreateSessionRequest } from "@goddard-ai/schema/daemon/sessions"
import type { DaemonSessionId } from "@goddard-ai/schema/id"
import { z } from "zod"

/** Pacing controls that bound how aggressively one loop may run. */
export const LoopRateLimits = z
  .strictObject({
    cycleDelay: z
      .string()
      .min(1)
      .optional()
      .describe("Delay between loop cycles, expressed as a duration string."),
    maxOpsPerMinute: z
      .number()
      .int()
      .positive()
      .optional()
      .describe("Maximum number of tool or model operations the loop may start per minute."),
    maxCyclesBeforePause: z
      .number()
      .int()
      .positive()
      .optional()
      .describe("Maximum number of completed loop cycles before the loop pauses itself."),
  })
  .describe("Rate limits that control how aggressively a loop may run.")

export type LoopRateLimits = z.infer<typeof LoopRateLimits>

/** Retry policy used when one loop prompt cycle fails. */
export const LoopRetryPolicy = z
  .strictObject({
    maxAttempts: z
      .number()
      .int()
      .positive()
      .optional()
      .describe("Maximum number of retry attempts after a loop operation fails."),
    initialDelayMs: z
      .number()
      .int()
      .nonnegative()
      .optional()
      .describe("Initial retry delay in milliseconds before the first retry."),
    maxDelayMs: z
      .number()
      .int()
      .nonnegative()
      .optional()
      .describe("Maximum retry delay in milliseconds after exponential backoff is applied."),
    backoffFactor: z
      .number()
      .positive()
      .optional()
      .describe("Multiplier applied to the retry delay after each failed prompt attempt."),
    jitterRatio: z
      .number()
      .nonnegative()
      .optional()
      .describe("Random jitter ratio applied to retry delays to avoid synchronized retries."),
  })
  .describe("Retry policy used when loop operations fail.")

export type LoopRetryPolicy = z.infer<typeof LoopRetryPolicy>

/** Persisted loop defaults loaded from root and packaged loop JSON config. */
export const LoopConfig = z
  .strictObject({
    session: StaticSessionParams.optional().describe(
      "Default session settings applied to loop-backed agent runs.",
    ),
    rateLimits: LoopRateLimits.optional().describe(
      "Loop pacing limits that bound runtime throughput.",
    ),
    retries: LoopRetryPolicy.optional().describe(
      "Retry settings used when a loop cycle or operation fails.",
    ),
  })
  .describe("Persisted loop defaults loaded from JSON.")

export type LoopConfig = z.infer<typeof LoopConfig>

/** Merges loop config layers using later layers as overrides. */
export function mergeLoopConfigLayers(...layers: Array<LoopConfig | undefined>) {
  const merged = mergeConfigLayers<LoopConfig>(layers)

  return LoopConfig.parse({
    ...merged,
    session: selectLast(layers, (layer) => layer?.session),
  })
}

const LoopSessionOverrides = CreateSessionRequest.omit({
  initialPrompt: true,
  oneShot: true,
}).partial()

/** Resolved loop rate limits with all required fields present. */
export const ResolvedLoopRateLimits = z.object({
  cycleDelay: z.string().min(1),
  maxOpsPerMinute: z.number().int().positive(),
  maxCyclesBeforePause: z.number().int().positive(),
})

export type ResolvedLoopRateLimits = z.infer<typeof ResolvedLoopRateLimits>

/** Resolved loop retry settings with all required fields present. */
export const ResolvedLoopRetries = z.object({
  maxAttempts: z.number().int().positive(),
  initialDelayMs: z.number().int().nonnegative(),
  maxDelayMs: z.number().int().nonnegative(),
  backoffFactor: z.number().positive(),
  jitterRatio: z.number().nonnegative(),
})

export type ResolvedLoopRetries = z.infer<typeof ResolvedLoopRetries>

export const ResolvedLoopSessionParams = LoopSessionOverrides.extend({
  agent: AgentSetting,
  mcpServers: z.array(McpServer),
  cwd: z.string(),
})

export type ResolvedLoopSessionParams = z.infer<typeof ResolvedLoopSessionParams>

/** Fully resolved loop config document used by runtime execution. */
export const ResolvedLoopConfig = z.object({
  session: ResolvedLoopSessionParams,
  rateLimits: ResolvedLoopRateLimits,
  retries: ResolvedLoopRetries,
})

export type ResolvedLoopConfig = z.infer<typeof ResolvedLoopConfig>

/** Request payload used to start or reuse one daemon-owned loop runtime. */
export const StartLoopRequest = z.strictObject({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
  session: LoopSessionOverrides.optional(),
  rateLimits: LoopRateLimits.optional(),
  retries: LoopRetryPolicy.optional(),
})

export type StartLoopRequest = z.infer<typeof StartLoopRequest>

/** Request payload used to fetch one daemon-owned loop runtime. */
export const GetLoopRequest = z.strictObject({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
})

export type GetLoopRequest = z.infer<typeof GetLoopRequest>

/** Request payload used to stop one daemon-owned loop runtime. */
export const ShutdownLoopRequest = z.strictObject({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
})

export type ShutdownLoopRequest = z.infer<typeof ShutdownLoopRequest>

/** Stable runtime states reported for daemon-managed loop hosts. */
export type DaemonLoopRuntimeState = "running"

/** Resolved session and pacing config owned by one daemon-managed loop runtime. */
export type DaemonLoopConfig = {
  promptModulePath: string
  session: Omit<CreateSessionRequest, "initialPrompt" | "oneShot">
  rateLimits: {
    cycleDelay: string
    maxOpsPerMinute: number
    maxCyclesBeforePause: number
  }
  retries: {
    maxAttempts: number
    initialDelayMs: number
    maxDelayMs: number
    backoffFactor: number
    jitterRatio: number
  }
}

/** Loop status summary exposed over daemon IPC. */
export type DaemonLoopStatus = {
  state: DaemonLoopRuntimeState
  rootDir: string
  loopName: string
  promptModulePath: string
  startedAt: string
  sessionId: string
  acpSessionId: string
  cycleCount: number
  lastPromptAt: string | null
}

/** One daemon-managed loop runtime addressed by repository root and loop name. */
export type DaemonLoop = DaemonLoopStatus & DaemonLoopConfig

/** Persisted association between one daemon loop runtime and its backing session. */
export const DaemonLoopSession = z.strictObject({
  sessionId: z.custom<DaemonSessionId>(),
  rootDir: z.string(),
  loopName: z.string(),
  promptModulePath: z.string(),
})

export type DaemonLoopSession = z.infer<typeof DaemonLoopSession>

/** Response payload returned when one loop runtime is fetched. */
export type GetLoopResponse = {
  loop: DaemonLoop
}

/** Response payload returned when one loop runtime is started. */
export type StartLoopResponse = {
  loop: DaemonLoop
}

/** Response payload returned when all running loop runtimes are listed. */
export type ListLoopsResponse = {
  loops: DaemonLoopStatus[]
}

/** Response payload returned after one loop runtime is stopped. */
export type ShutdownLoopResponse = {
  rootDir: string
  loopName: string
  success: boolean
}

import { z } from "zod"
import { DaemonSessionMetadata } from "./daemon/session-metadata.js"
import { AgentDistribution } from "./session-server.js"

const stringRecord = z.record(z.string(), z.string())

const sessionAgent = z.union([z.string().min(1), AgentDistribution])

const sessionConfig = z
  .object({
    agent: sessionAgent.optional(),
    cwd: z.string().min(1).optional(),
    mcpServers: z.array(z.unknown()).optional(),
    systemPrompt: z.string().min(1).optional(),
    env: stringRecord.optional(),
    metadata: DaemonSessionMetadata.optional(),
  })
  .passthrough()

const loopRateLimits = z
  .object({
    cycleDelay: z.string().min(1).optional(),
    maxOpsPerMinute: z.number().int().positive().optional(),
    maxCyclesBeforePause: z.number().int().positive().optional(),
  })
  .passthrough()

const loopRetries = z
  .object({
    maxAttempts: z.number().int().positive().optional(),
    initialDelayMs: z.number().int().nonnegative().optional(),
    maxDelayMs: z.number().int().nonnegative().optional(),
    backoffFactor: z.number().positive().optional(),
    jitterRatio: z.number().nonnegative().optional(),
  })
  .passthrough()

/** Schema for persisted action defaults loaded from JSON. */
export const ActionConfig = sessionConfig

/** Schema for persisted loop defaults loaded from JSON. */
export const LoopConfig = z
  .object({
    session: sessionConfig.optional(),
    rateLimits: loopRateLimits.optional(),
    retries: loopRetries.optional(),
  })
  .passthrough()

/** Schema for the shared root config document. */
export const RootConfig = z
  .object({
    actions: ActionConfig.optional(),
    loops: LoopConfig.optional(),
  })
  .passthrough()

/** Schema for resolved loop rate limits with all required fields present. */
export const ResolvedLoopRateLimits = z.object({
  cycleDelay: z.string().min(1),
  maxOpsPerMinute: z.number().int().positive(),
  maxCyclesBeforePause: z.number().int().positive(),
})

/** Schema for resolved loop retry settings with all required fields present. */
export const ResolvedLoopRetries = z.object({
  maxAttempts: z.number().int().positive(),
  initialDelayMs: z.number().int().nonnegative(),
  maxDelayMs: z.number().int().nonnegative(),
  backoffFactor: z.number().positive(),
  jitterRatio: z.number().nonnegative(),
})

/** Schema for a fully resolved loop config document. */
export const ResolvedLoopConfig = z.object({
  session: sessionConfig.extend({
    agent: sessionAgent,
    cwd: z.string().min(1),
    mcpServers: z.array(z.unknown()),
  }),
  rateLimits: ResolvedLoopRateLimits,
  retries: ResolvedLoopRetries,
})

export type GoddardActionConfigDocument = z.infer<typeof ActionConfig>

export type GoddardLoopConfigDocument = z.infer<typeof LoopConfig>

export type GoddardRootConfigDocument = z.infer<typeof RootConfig>

export type GoddardLoopRateLimitsConfig = z.infer<typeof ResolvedLoopRateLimits>

export type GoddardLoopRetriesConfig = z.infer<typeof ResolvedLoopRetries>

export type ResolvedGoddardLoopConfigDocument = z.infer<typeof ResolvedLoopConfig>

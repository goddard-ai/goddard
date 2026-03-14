import type { SessionParams } from "./session-server.js"
import type * as acp from "@agentclientprotocol/sdk"

export interface LoopContext {
  cycleNumber: number
  lastSummary?: string
}

export type LoopStrategy = {
  nextPrompt(ctx: LoopContext): string
}

export type AgentLoopSessionParams = SessionParams & { oneShot?: undefined }

export interface AgentLoopRateLimits {
  cycleDelay?: string
  maxTokensPerCycle?: number
  maxOpsPerMinute?: number
  maxCyclesBeforePause?: number
}

export interface AgentLoopRetryConfig {
  maxAttempts: number
  initialDelayMs: number
  maxDelayMs: number
  backoffFactor: number
  jitterRatio: number
  retryableErrors: (
    error: unknown,
    context: { cycle: number; attempt: number; maxAttempts: number },
  ) => boolean
}

export interface AgentLoopParams {
  session: AgentLoopSessionParams
  strategy: LoopStrategy
  rateLimits?: AgentLoopRateLimits
  retries: AgentLoopRetryConfig
}

export type AgentLoopHandler = acp.Client

import type * as acp from "@agentclientprotocol/sdk"
import type { SessionParams } from "./session-server.ts"

/** Session configuration allowed for loop-managed daemon sessions. */
export type AgentLoopSessionParams = Omit<
  Extract<SessionParams, { oneShot?: undefined }>,
  "initialPrompt"
>

/** Pacing limits enforced around repeated loop cycles. */
export interface AgentLoopRateLimits {
  /** Minimum pause between completed loop cycles. */
  cycleDelay: string
  /** Maximum number of cycle operations allowed in a rolling minute. */
  maxOpsPerMinute: number
  /** Number of completed cycles before the loop sleeps for 24 hours. */
  maxCyclesBeforePause: number
}

/** Retry policy applied when one loop cycle fails to deliver its prompt. */
export interface AgentLoopRetryConfig {
  /** Maximum number of prompt attempts for a single cycle. */
  maxAttempts: number
  /** Delay before the first retry attempt, in milliseconds. */
  initialDelayMs: number
  /** Upper bound for retry backoff delay, in milliseconds. */
  maxDelayMs: number
  /** Exponential multiplier applied to each successive retry delay. */
  backoffFactor: number
  /** Fractional jitter applied to retry delays. */
  jitterRatio: number
  /** Predicate that decides whether a prompt error should be retried. */
  retryableErrors: (
    error: unknown,
    context: { cycle: number; attempt: number; maxAttempts: number },
  ) => boolean
}

/** Full configuration for one repeated daemon-backed agent loop. */
export interface AgentLoopParams {
  /** Function that produces the next prompt to send to the agent. */
  nextPrompt: () => string
  /** Session configuration used to start the underlying agent session. */
  session: AgentLoopSessionParams
  /** Cycle pacing and pause controls enforced by the loop runtime. */
  rateLimits: AgentLoopRateLimits
  /** Retry policy applied when prompting the agent fails. */
  retries: AgentLoopRetryConfig
}

/** ACP client hooks that can observe or respond to loop-driven session activity. */
export type AgentLoopHandler = acp.Client

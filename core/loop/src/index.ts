import type { AgentLoopHandler, AgentLoopParams } from "@goddard-ai/schema/loop"
import { runAgent, type AgentSession } from "@goddard-ai/session"
import exitHook from "exit-hook"
import { proportionalJitter, sleep } from "radashi"
import { RateLimiter } from "./rate-limiter.ts"

export async function runAgentLoop(
  { nextPrompt, session: sessionParams, rateLimits, retries }: AgentLoopParams,
  handler?: AgentLoopHandler,
): Promise<AgentSession> {
  const session = await runAgent(sessionParams, handler)

  const rateLimiter = new RateLimiter({
    cycleDelay: rateLimits.cycleDelay,
    maxOpsPerMinute: rateLimits.maxOpsPerMinute,
  })

  const endlessLoop = async (): Promise<void> => {
    let cycleCount = 0

    const removeExitHook = exitHook(() => {
      void session.stop()
    })

    try {
      while (true) {
        cycleCount += 1

        const promptMessage = nextPrompt()

        let attempt = 0
        while (true) {
          try {
            await session.prompt(promptMessage)
            break
          } catch (error) {
            if (
              error instanceof Error &&
              (error.name === "AbortError" || error.message.toLowerCase().includes("abort"))
            ) {
              return
            }

            attempt += 1

            if (
              attempt >= retries.maxAttempts ||
              !retries.retryableErrors(error, {
                cycle: cycleCount,
                attempt,
                maxAttempts: retries.maxAttempts,
              })
            ) {
              throw error
            }

            const baseDelay = Math.min(
              retries.maxDelayMs,
              Math.round(retries.initialDelayMs * Math.pow(retries.backoffFactor, attempt - 1)),
            )
            const retryDelay = proportionalJitter(baseDelay, retries.jitterRatio)

            await sleep(retryDelay)
          }
        }

        await rateLimiter.throttle()

        if (cycleCount % rateLimits.maxCyclesBeforePause === 0) {
          await sleep(24 * 60 * 60 * 1000)
        }
      }
    } finally {
      removeExitHook()
    }
  }

  // Start the background loop immediately
  endlessLoop().catch(console.error)

  return session
}

export { Models, type Model } from "@goddard-ai/config"
export type {
  AgentLoopHandler,
  AgentLoopParams,
  AgentLoopRateLimits,
  AgentLoopRetryConfig,
  AgentLoopSessionParams,
} from "@goddard-ai/schema/loop"
export { LOOP_SYSTEM_PROMPT } from "./prompts.ts"
export type { GoddardLoopConfig, PiAgentConfig } from "./types.ts"

import exitHook from "exit-hook"
import { proportionalJitter, sleep } from "radashi"
import type { AgentLoopHandler, AgentLoopParams } from "@goddard-ai/schema/loop"
import type { AgentSession } from "../daemon/session/client-session.js"
import { runAgent, type RunAgentOptions } from "../daemon/session/client.js"
import { RateLimiter } from "./rate-limiter.js"

/** Starts a daemon-backed loop that keeps prompting the same session under retry and pacing rules. */
export async function runAgentLoop(
  { nextPrompt, session: sessionParams, rateLimits, retries }: AgentLoopParams,
  handler?: AgentLoopHandler,
  options?: RunAgentOptions,
): Promise<AgentSession> {
  const session = await runAgent(sessionParams, handler, options)

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
            await sleep(proportionalJitter(baseDelay, retries.jitterRatio))
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

  void endlessLoop().catch(console.error)

  return session
}

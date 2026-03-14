import type { GoddardLoopConfig } from "./types.ts"
import { configSchema } from "./types.ts"
import { RateLimiter } from "./rate-limiter.ts"
import { runAgent, type AgentSession } from "@goddard-ai/session"
import type * as acp from "@agentclientprotocol/sdk"
import type {
  AgentLoopHandler,
  AgentLoopParams,
  AgentLoopRetryConfig,
  LoopStrategy,
} from "@goddard-ai/schema/loop"
import { join } from "node:path"
import { existsSync } from "node:fs"
import { homedir } from "node:os"

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function withJitter(delayMs: number, jitterRatio: number): number {
  if (jitterRatio <= 0) {
    return delayMs
  }

  const min = Math.max(0, delayMs * (1 - jitterRatio))
  const max = delayMs * (1 + jitterRatio)
  return Math.round(min + Math.random() * (max - min))
}

function isDoneSignal(text: string | undefined): boolean {
  if (!text) {
    return false
  }

  const normalized = text.trim()
  if (normalized.toUpperCase() === "DONE") {
    return true
  }

  if (/^SUMMARY\s*\|\s*DONE$/i.test(normalized)) {
    return true
  }

  return /(^|\n)\s*DONE\s*$/i.test(text)
}

export async function runAgentLoop(
  { session: sessionParams, strategy, rateLimits, retries }: AgentLoopParams,
  handler?: AgentLoopHandler,
): Promise<AgentSession> {
  const status = {
    cycle: 0,
    tokensUsed: 0,
    uptime: 0,
    startTime: Date.now(),
  }

  const session = await runAgent(sessionParams, handler)

  const endlessLoop = async (): Promise<void> => {
    let lastSummary: string | undefined

    status.cycle = 0
    status.tokensUsed = 0
    status.uptime = 0
    status.startTime = Date.now()

    const onSigint = () => {
      session.stop()
      process.exit(0)
    }
    process.on("SIGINT", onSigint)

    try {
      while (true) {
        status.cycle += 1
        status.uptime = Date.now() - status.startTime

        const promptMessage = strategy.nextPrompt({
          cycleNumber: status.cycle,
          lastSummary,
        })

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
                cycle: status.cycle,
                attempt,
                maxAttempts: retries.maxAttempts,
              })
            ) {
              throw error
            }

            const baseDelay = Math.min(
              retries.maxDelayMs,
              Math.round(
                retries.initialDelayMs * Math.pow(retries.backoffFactor, attempt - 1),
              ),
            )
            const retryDelay = withJitter(baseDelay, retries.jitterRatio)

            await sleep(retryDelay)
          }
        }

        const history = await session.getHistory()
        const lastMessage = history[history.length - 1]
        const assistantText = extractAssistantText(lastMessage)

        lastSummary = assistantText.length > 0 ? assistantText : `Completed cycle ${status.cycle}`

        if (isDoneSignal(lastSummary)) {
          return
        }
      }
    } finally {
      process.removeListener("SIGINT", onSigint)
    }
  }

  // Start the background loop immediately
  endlessLoop().catch(console.error)

  return session
}

function extractAssistantText(message: unknown): string {
  if (!isAssistantMessage(message)) {
    return ""
  }

  const textBlock = message.content.find(isTextContentBlock)
  return textBlock?.text ?? ""
}

function isAssistantMessage(
  value: unknown,
): value is { role: "assistant"; content: unknown[] } {
  return (
    typeof value === "object" &&
    value !== null &&
    "role" in value &&
    value.role === "assistant" &&
    "content" in value &&
    Array.isArray(value.content)
  )
}

function isTextContentBlock(value: unknown): value is { type: "text"; text: string } {
  return (
    typeof value === "object" &&
    value !== null &&
    "type" in value &&
    value.type === "text" &&
    "text" in value &&
    typeof value.text === "string"
  )
}

export function createGoddardConfig(config: GoddardLoopConfig): GoddardLoopConfig {
  return config
}

export type { GoddardLoopConfig, PiAgentConfig } from "./types.ts"
export type {
  AgentLoopHandler,
  AgentLoopParams,
  AgentLoopRateLimits,
  AgentLoopRetryConfig,
  AgentLoopSessionParams,
  LoopContext,
  LoopStrategy,
} from "@goddard-ai/schema/loop"
export { DefaultStrategy } from "./strategies.ts"
export { Models, type Model } from "@goddard-ai/config"
export { LOOP_SYSTEM_PROMPT } from "./prompts.ts"

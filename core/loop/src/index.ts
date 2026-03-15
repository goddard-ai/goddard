import type { AgentLoopHandler, AgentLoopParams } from "@goddard-ai/schema/loop"
import { runAgent, type AgentSession } from "@goddard-ai/session"
import {
  AuthStorage,
  InteractiveMode,
  ModelRegistry,
  createAgentSession,
} from "@mariozechner/pi-coding-agent"
import exitHook from "exit-hook"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { join } from "node:path"
import { proportionalJitter, sleep } from "radashi"
import { RateLimiter } from "./rate-limiter.ts"
import type { GoddardLoop } from "./types.ts"

export async function runAgentLoop(
  { nextPrompt, session: sessionParams, rateLimits, retries }: AgentLoopParams,
  handler?: AgentLoopHandler,
): Promise<AgentSession> {
  const session = (await runAgent(sessionParams as any, handler)) as AgentSession | null

  if (!session) {
    throw new Error("Expected a persistent agent session for loop execution")
  }

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

  endlessLoop().catch(console.error)

  return session
}

function resolveAgentDir(configuredDir?: string): string | undefined {
  if (configuredDir) {
    if (configuredDir.startsWith("~/")) {
      return join(homedir(), configuredDir.slice(2))
    }
    return configuredDir
  }

  const defaultAgentDir = join(homedir(), ".pi", "agent")
  return existsSync(defaultAgentDir) ? defaultAgentDir : undefined
}

function resolveConfiguredModel(modelRef: string, agentDir?: string) {
  const authPath = agentDir ? join(agentDir, "auth.json") : undefined
  const modelsPath = agentDir ? join(agentDir, "models.json") : undefined

  const authStorage = AuthStorage.create(authPath)
  const modelRegistry = new ModelRegistry(authStorage, modelsPath)

  if (modelRef.includes("/")) {
    const [provider, ...idParts] = modelRef.split("/")
    const modelId = idParts.join("/")

    if (!provider || !modelId) {
      throw new Error(`Invalid model format "${modelRef}". Use "provider/modelId" or "modelId".`)
    }

    const model = modelRegistry.find(provider, modelId)
    if (!model) {
      throw new Error(`Unknown model "${modelRef}". Verify provider/modelId in pi-coding-agent models.`)
    }

    return model
  }

  const matches = modelRegistry.getAll().filter((model) => model.id === modelRef)

  if (matches.length === 0) {
    throw new Error(`Unknown model id "${modelRef}". Use "provider/modelId" for explicit selection.`)
  }

  if (matches.length > 1) {
    const options = matches.map((model) => `${model.provider}/${model.id}`).join(", ")
    throw new Error(`Ambiguous model id "${modelRef}". Use one of: ${options}`)
  }

  return matches[0]
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

export interface LoopRuntimeConfig {
  agent: string
  cwd: string
  systemPrompt: string
  strategy?: string
  mcpServers?: any[]
}

export function createLoop(config: LoopRuntimeConfig): GoddardLoop {
  const limiter = new RateLimiter({
    cycleDelay: "30m",
    maxOpsPerMinute: 120,
  })
  const retryConfig = {
    maxAttempts: 3,
    initialDelayMs: 1000,
    maxDelayMs: 30000,
    backoffFactor: 2,
    jitterRatio: 0.2,
  }
  const rateLimits = {
    maxTokensPerCycle: 128000,
    maxCyclesBeforePause: 100,
  }

  const status = {
    cycle: 0,
    tokensUsed: 0,
    startTime: Date.now(),
  }

  const endlessLoop = async (): Promise<void> => {
    let lastSummary: string | undefined

    status.cycle = 0
    status.tokensUsed = 0
    status.startTime = Date.now()

    const resolvedAgentDir = resolveAgentDir()
    const configuredModel = resolveConfiguredModel(config.agent, resolvedAgentDir)

    const { session } = await createAgentSession({
      cwd: config.cwd,
      model: configuredModel,
      thinkingLevel: "medium",
      agentDir: resolvedAgentDir,
    })

    const ui = new InteractiveMode(session)
    await ui.init()

    const onSigint = () => {
      ui.stop()
      process.exit(0)
    }
    process.on("SIGINT", onSigint)

    try {
      while (true) {
        status.cycle += 1

        const countdownPause = async (delayMs: number) => {
          ui.showWarning(
            `Rate limit reached. Pausing loop for ${Math.round(delayMs / 1000)} seconds...`,
          )
          await sleep(delayMs)
        }

        if (status.cycle > 1) {
          await limiter.throttle(countdownPause)
        }

        if (status.cycle % rateLimits.maxCyclesBeforePause === 0) {
          await countdownPause(24 * 60 * 60 * 1000)
        }

        const prompt = `Cycle ${status.cycle}. Last: ${lastSummary ?? "none"}. ${config.systemPrompt}`
        const before = session.getSessionStats().tokens.total

        let attempt = 0
        while (true) {
          try {
            await session.sendUserMessage(prompt)
            break
          } catch (error) {
            if (
              error instanceof Error &&
              (error.name === "AbortError" || error.message.toLowerCase().includes("abort"))
            ) {
              return
            }

            attempt += 1
            if (attempt >= retryConfig.maxAttempts) {
              throw error
            }

            const baseDelay = Math.min(
              retryConfig.maxDelayMs,
              Math.round(
                retryConfig.initialDelayMs * Math.pow(retryConfig.backoffFactor, attempt - 1),
              ),
            )
            const retryDelay = proportionalJitter(baseDelay, retryConfig.jitterRatio)

            await sleep(retryDelay)
          }
        }

        const stats = session.getSessionStats()
        const cycleTokens = stats.tokens.total - before
        if (cycleTokens > rateLimits.maxTokensPerCycle) {
          throw new Error(
            `[goddard loop] Cycle ${status.cycle} exceeded maxTokensPerCycle: used ${cycleTokens}, limit ${rateLimits.maxTokensPerCycle}`,
          )
        }

        status.tokensUsed = stats.tokens.total
        lastSummary = session.getLastAssistantText() || `Completed cycle ${status.cycle}`

        if (isDoneSignal(lastSummary)) {
          return
        }
      }
    } finally {
      ui.stop()
      process.removeListener("SIGINT", onSigint)
    }
  }

  let isRunning = false

  return {
    start: async () => {
      if (isRunning) {
        throw new Error("Loop is already running")
      }

      isRunning = true
      try {
        await endlessLoop()
      } finally {
        isRunning = false
      }
    },
    get status() {
      return {
        cycle: status.cycle,
        tokensUsed: status.tokensUsed,
        uptime: Date.now() - status.startTime,
      }
    },
  }
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
export { DefaultStrategy } from "./strategies.ts"
export type { CycleContext, CycleStrategy } from "./strategies.ts"
export type { GoddardLoop, GoddardLoopConfig, LoopStatus, PiAgentConfig } from "./types.ts"

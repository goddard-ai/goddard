import { pathToFileURL } from "node:url"
import type { DaemonLogger, DaemonLogService } from "@goddard-ai/daemon-plugin"
import * as acp from "acp-client/protocol"
import { getErrorMessage, proportionalJitter } from "radashi"

import type { DaemonLoop, DaemonLoopStatus } from "../schema.ts"
import { LoopContext } from "./context.ts"
import { LoopRateLimiter } from "./rate-limiter.ts"
import type { ResolvedLoopStartRequest } from "./resolver.ts"

const LOOP_PAUSE_INTERVAL_MS = 24 * 60 * 60 * 1000

/** Detects the configured cycle boundary where the loop intentionally pauses for a full day. */
function shouldPauseLoop(cycleCount: number, maxCyclesBeforePause: number): boolean {
  return cycleCount % maxCyclesBeforePause === 0
}

/** Runtime dependencies shared by one daemon-owned loop host. */
export interface LoopRuntimeDeps {
  log: DaemonLogService
  session: {
    newSession: (params: { request: ResolvedLoopStartRequest["session"] }) => Promise<{
      id: `ses_${string}`
      acpSessionId: string
    }>
    promptSession: (
      id: `ses_${string}`,
      prompt: string | acp.ContentBlock[],
    ) => Promise<acp.PromptResponse>
    shutdownSession: (id: `ses_${string}`) => Promise<boolean>
  }
  onStop?: (input: { rootDir: string; loopName: string }) => void
}

/** Daemon-owned loop runtime backed by one persistent daemon session. */
export class LoopRuntime {
  readonly #config: ResolvedLoopStartRequest
  readonly #deps: LoopRuntimeDeps
  readonly #startedAt: string
  readonly #sessionId: `ses_${string}`
  readonly #sessionAcpId: string
  readonly #context: LoopContext
  readonly #rateLimiter: LoopRateLimiter
  readonly #logger: DaemonLogger

  #cycleCount = 0
  #lastPromptAt: string | null = null
  #runTask: Promise<void> | null = null
  #sleepHandle: ReturnType<typeof setTimeout> | null = null
  #stopped = false
  #shutdownCompleted = false
  #stoppedNotified = false

  private constructor(input: {
    config: ResolvedLoopStartRequest
    deps: LoopRuntimeDeps
    sessionId: `ses_${string}`
    sessionAcpId: string
    logger: DaemonLogger
  }) {
    this.#config = input.config
    this.#deps = input.deps
    this.#sessionId = input.sessionId
    this.#sessionAcpId = input.sessionAcpId
    this.#logger = input.logger
    this.#context = {
      rootDir: input.config.rootDir,
      loopName: input.config.loopName,
      sessionId: input.sessionId,
      acpSessionId: input.sessionAcpId,
    }
    this.#startedAt = new Date().toISOString()
    this.#rateLimiter = new LoopRateLimiter({
      cycleDelay: input.config.rateLimits.cycleDelay,
      maxOpsPerMinute: input.config.rateLimits.maxOpsPerMinute,
    })
  }

  /** Starts one daemon-owned loop runtime and begins background cycle execution. */
  static async start(
    config: ResolvedLoopStartRequest,
    deps: LoopRuntimeDeps,
  ): Promise<LoopRuntime> {
    const logger = deps.log.createLogger()
    const session = await deps.session.newSession({
      request: {
        ...config.session,
        systemPrompt: config.session.systemPrompt ?? "",
        worktree: config.session.worktree ?? { enabled: true },
      },
    })

    const runtime = new LoopRuntime({
      config,
      deps,
      sessionId: session.id,
      sessionAcpId: session.acpSessionId,
      logger,
    })

    LoopContext.run(runtime.#context, () => {
      logger.log("loop.runtime_started", {
        promptModulePath: config.promptModulePath,
      })
    })

    runtime.#runTask = LoopContext.run(runtime.#context, () =>
      runtime.#run().catch(async (error) => {
        if (!runtime.#stopped) {
          logger.log("loop.runtime_failed", {
            errorMessage: getErrorMessage(error),
          })
          runtime.#stopped = true
          await runtime.#shutdownLoopRuntime()
        }
        throw error
      }),
    )
    // Suppress the detached task warning here because failures are already logged and surfaced via runtime state.
    runtime.#runTask?.catch(() => {})
    return runtime
  }

  /** Returns the full daemon loop record exposed by start and get calls. */
  getLoop(): DaemonLoop {
    return {
      ...this.getStatus(),
      promptModulePath: this.#config.promptModulePath,
      session: this.#config.session,
      rateLimits: this.#config.rateLimits,
      retries: this.#config.retries,
    }
  }

  /** Returns the current public runtime status for one daemon-owned loop. */
  getStatus(): DaemonLoopStatus {
    return {
      state: "running",
      rootDir: this.#config.rootDir,
      loopName: this.#config.loopName,
      promptModulePath: this.#config.promptModulePath,
      startedAt: this.#startedAt,
      sessionId: this.#sessionId,
      acpSessionId: this.#sessionAcpId,
      cycleCount: this.#cycleCount,
      lastPromptAt: this.#lastPromptAt,
    }
  }

  /** Stops the loop runtime and shuts down its backing daemon session. */
  async stop(): Promise<void> {
    await LoopContext.run(this.#context, async () => {
      this.#stopped = true
      await this.#shutdownLoopRuntime()
      await this.#runTask?.catch(() => {})
    })
  }

  /** Runs the daemon-owned loop until it completes, fails, or is stopped. */
  async #run(): Promise<void> {
    const nextPrompt = await importNextPrompt(this.#config.promptModulePath)

    while (!this.#stopped) {
      this.#cycleCount += 1
      const response = await this.#promptWithRetries(nextPrompt())
      if (this.#stopped) {
        return
      }

      if (response.stopReason === "end_turn") {
        this.#stopped = true
        await this.#shutdownLoopRuntime()
        return
      }

      await this.#rateLimiter.throttle(async (ms) => this.#sleep(ms))
      if (this.#stopped) {
        return
      }

      if (shouldPauseLoop(this.#cycleCount, this.#config.rateLimits.maxCyclesBeforePause)) {
        // Insert a long pause so unattended loops yield after each configured burst of cycles.
        await this.#sleep(LOOP_PAUSE_INTERVAL_MS)
      }
    }
  }

  /** Prompts the active daemon session with the configured retry policy. */
  async #promptWithRetries(promptMessage: string): Promise<acp.PromptResponse> {
    let attempt = 0

    while (true) {
      if (this.#stopped) {
        throw new Error("Loop runtime stopped before prompt execution")
      }

      try {
        this.#lastPromptAt = new Date().toISOString()
        const response = await this.#deps.session.promptSession(this.#sessionId, promptMessage)
        this.#logger.log("loop.prompt_completed", {
          cycleCount: this.#cycleCount,
          stopReason: response.stopReason,
          prompt: this.#deps.log.createPayloadPreview(promptMessage),
        })
        return response
      } catch (error) {
        attempt += 1
        if (attempt >= this.#config.retries.maxAttempts || !isRetryableLoopError(error)) {
          throw error
        }

        const baseDelay = Math.min(
          this.#config.retries.maxDelayMs,
          Math.round(
            this.#config.retries.initialDelayMs *
              Math.pow(this.#config.retries.backoffFactor, attempt - 1),
          ),
        )
        await this.#sleep(proportionalJitter(baseDelay, this.#config.retries.jitterRatio))
      }
    }
  }

  /** Sleeps between loop cycles while remaining interruptible by daemon shutdown. */
  async #sleep(ms: number): Promise<void> {
    if (this.#stopped || ms <= 0) {
      return
    }

    await new Promise<void>((resolve) => {
      this.#sleepHandle = setTimeout(() => {
        this.#sleepHandle = null
        resolve()
      }, ms)
    })
  }

  /** Performs one-time runtime shutdown side effects without awaiting the active loop task. */
  async #shutdownLoopRuntime(): Promise<void> {
    await LoopContext.run(this.#context, async () => {
      if (this.#shutdownCompleted) {
        return
      }

      this.#shutdownCompleted = true
      if (this.#sleepHandle) {
        clearTimeout(this.#sleepHandle)
        this.#sleepHandle = null
      }
      await this.#deps.session.shutdownSession(this.#sessionId).catch(() => {})
      this.#logger.log("loop.runtime_stopped", {
        cycleCount: this.#cycleCount,
      })
      this.#notifyStopped()
    })
  }

  /** Emits the manager stop callback only once per runtime lifecycle. */
  #notifyStopped(): void {
    if (this.#stoppedNotified) {
      return
    }

    this.#stoppedNotified = true
    this.#deps.onStop?.({
      rootDir: this.#config.rootDir,
      loopName: this.#config.loopName,
    })
  }
}

/** Loads the packaged loop prompt module and returns its exported prompt function. */
async function importNextPrompt(promptModulePath: string): Promise<() => string> {
  const promptModule = await import(pathToFileURL(promptModulePath).href)
  if (!("nextPrompt" in promptModule) || typeof promptModule.nextPrompt !== "function") {
    throw new Error(`Loop prompt module "${promptModulePath}" must export a callable nextPrompt.`)
  }

  return promptModule.nextPrompt as () => string
}

/** Returns true when the loop should retry a failed prompt operation. */
function isRetryableLoopError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.name === "AbortError" ||
      error.message.toLowerCase().includes("abort") ||
      error.message.toLowerCase().includes("closed"))
  )
}

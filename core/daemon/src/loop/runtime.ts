import * as acp from "@agentclientprotocol/sdk"
import type { DaemonLoop, DaemonLoopStatus } from "@goddard-ai/schema/daemon"
import type { StartDaemonLoopRequest } from "@goddard-ai/schema/daemon/loops"
import { proportionalJitter } from "radashi"
import { pathToFileURL } from "node:url"
import { createDaemonLogger, createPayloadPreview } from "../logging.js"
import type { SessionManager } from "../session/index.js"
import { LoopRateLimiter } from "./rate-limiter.js"

const logger = createDaemonLogger()

/** Runtime dependencies shared by one daemon-owned loop host. */
export interface LoopRuntimeDeps {
  sessionManager: SessionManager
  onStop?: (input: { rootDir: string; loopName: string }) => void
}

/** Daemon-owned loop runtime backed by one persistent daemon session. */
export class LoopRuntime {
  readonly #config: StartDaemonLoopRequest
  readonly #deps: LoopRuntimeDeps
  readonly #startedAt: string
  readonly #sessionId: string
  readonly #sessionAcpId: string
  readonly #rateLimiter: LoopRateLimiter

  #cycleCount = 0
  #lastPromptAt: string | null = null
  #runTask: Promise<void> | null = null
  #sleepHandle: ReturnType<typeof setTimeout> | null = null
  #stopped = false
  #shutdownCompleted = false
  #stoppedNotified = false

  private constructor(input: {
    config: StartDaemonLoopRequest
    deps: LoopRuntimeDeps
    sessionId: string
    sessionAcpId: string
  }) {
    this.#config = input.config
    this.#deps = input.deps
    this.#sessionId = input.sessionId
    this.#sessionAcpId = input.sessionAcpId
    this.#startedAt = new Date().toISOString()
    this.#rateLimiter = new LoopRateLimiter({
      cycleDelay: input.config.rateLimits.cycleDelay,
      maxOpsPerMinute: input.config.rateLimits.maxOpsPerMinute,
    })
  }

  /** Starts one daemon-owned loop runtime and begins background cycle execution. */
  static async start(config: StartDaemonLoopRequest, deps: LoopRuntimeDeps): Promise<LoopRuntime> {
    const session = await deps.sessionManager.createSession({
      ...config.session,
      systemPrompt: config.session.systemPrompt ?? "",
      metadata: {
        ...config.session.metadata,
        loop: {
          rootDir: config.rootDir,
          loopName: config.loopName,
          promptModulePath: config.promptModulePath,
        },
      },
    })

    const runtime = new LoopRuntime({
      config,
      deps,
      sessionId: session.id,
      sessionAcpId: session.acpId,
    })

    logger.log("loop.runtime_started", {
      rootDir: config.rootDir,
      loopName: config.loopName,
      sessionId: session.id,
      acpId: session.acpId,
      promptModulePath: config.promptModulePath,
    })

    runtime.#runTask = runtime.#run().catch(async (error) => {
      if (!runtime.#stopped) {
        logger.log("loop.runtime_failed", {
          rootDir: config.rootDir,
          loopName: config.loopName,
          sessionId: runtime.#sessionId,
          errorMessage: error instanceof Error ? error.message : String(error),
        })
        runtime.#stopped = true
        await runtime.#shutdownLoopRuntime()
      }
      throw error
    })
    runtime.#runTask.catch(() => {})
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
      acpId: this.#sessionAcpId,
      cycleCount: this.#cycleCount,
      lastPromptAt: this.#lastPromptAt,
    }
  }

  /** Stops the loop runtime and shuts down its backing daemon session. */
  async stop(): Promise<void> {
    this.#stopped = true
    await this.#shutdownLoopRuntime()
    await this.#runTask?.catch(() => {})
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

      if (this.#cycleCount % this.#config.rateLimits.maxCyclesBeforePause === 0) {
        await this.#sleep(24 * 60 * 60 * 1000)
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
        const response = await this.#deps.sessionManager.promptSession(
          this.#sessionId,
          promptMessage,
        )
        logger.log("loop.prompt_completed", {
          rootDir: this.#config.rootDir,
          loopName: this.#config.loopName,
          sessionId: this.#sessionId,
          cycleCount: this.#cycleCount,
          stopReason: response.stopReason,
          prompt: createPayloadPreview(promptMessage),
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
    if (this.#shutdownCompleted) {
      return
    }

    this.#shutdownCompleted = true
    if (this.#sleepHandle) {
      clearTimeout(this.#sleepHandle)
      this.#sleepHandle = null
    }
    await this.#deps.sessionManager.shutdownSession(this.#sessionId).catch(() => {})
    logger.log("loop.runtime_stopped", {
      rootDir: this.#config.rootDir,
      loopName: this.#config.loopName,
      sessionId: this.#sessionId,
      cycleCount: this.#cycleCount,
    })
    this.#notifyStopped()
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

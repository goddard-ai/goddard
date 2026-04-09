import { IpcClientError } from "@goddard-ai/ipc"
import type { WorkforceDescription } from "@goddard-ai/schema/daemon"
import type { WorkforceStatus } from "@goddard-ai/schema/workforce"
import type { WorkforceActorContext } from "../context.ts"
import { createLogger } from "../logging.ts"
import { normalizeWorkforceRootDir } from "./paths.ts"
import { WorkforceRuntime, type WorkforceRuntimeDeps } from "./runtime.ts"

/** Supported daemon-side workforce mutations routed over IPC or agent tools. */
export type WorkforceManagerMutation =
  | {
      type: "request"
      targetAgentId: string
      input: string
      intent?: "default" | "create"
    }
  | {
      type: "update"
      requestId: string
      input: string
    }
  | {
      type: "cancel"
      requestId: string
      reason: string | null
    }
  | {
      type: "truncate"
      agentId: string | null
      reason: string | null
    }
  | {
      type: "respond"
      requestId: string
      output: string
    }
  | {
      type: "suspend"
      requestId: string
      reason: string
    }

/** Optional lifecycle dependencies used to build new runtime instances. */
export interface WorkforceManagerDeps extends WorkforceRuntimeDeps {
  createRuntime?: (rootDir: string, deps: WorkforceRuntimeDeps) => Promise<WorkforceRuntime>
}

/** Daemon-owned runtime registry keyed by normalized repository root. */
export interface WorkforceManager {
  startWorkforce: (rootDir: string) => Promise<WorkforceDescription>
  getWorkforce: (rootDir: string) => Promise<WorkforceDescription>
  listWorkforces: () => Promise<WorkforceStatus[]>
  shutdownWorkforce: (rootDir: string) => Promise<boolean>
  appendWorkforceEvent: (
    rootDir: string,
    mutation: WorkforceManagerMutation,
    actor?: WorkforceActorContext,
  ) => Promise<{ workforce: WorkforceStatus; requestId: string | null }>
  close: () => Promise<void>
}

export function createWorkforceManager(deps: WorkforceManagerDeps): WorkforceManager {
  const logger = createLogger()
  const runtimes = new Map<string, WorkforceRuntime>()

  async function getRuntime(
    rootDir: string,
  ): Promise<{ rootDir: string; runtime: WorkforceRuntime }> {
    const normalizedRootDir = await normalizeWorkforceRootDir(rootDir)
    const runtime = runtimes.get(normalizedRootDir)
    if (!runtime) {
      throw new IpcClientError(`No workforce is running for ${normalizedRootDir}`)
    }

    return {
      rootDir: normalizedRootDir,
      runtime,
    }
  }

  return {
    async startWorkforce(rootDir: string): Promise<WorkforceDescription> {
      const normalizedRootDir = await normalizeWorkforceRootDir(rootDir)
      const existing = runtimes.get(normalizedRootDir)
      if (existing) {
        logger.log("workforce.runtime_reused", {
          rootDir: normalizedRootDir,
        })
        return existing.getWorkforce()
      }

      let runtime: WorkforceRuntime
      try {
        runtime = await (deps.createRuntime ?? WorkforceRuntime.start)(normalizedRootDir, {
          sessionManager: deps.sessionManager,
          runSession: deps.runSession,
        })
      } catch (error) {
        logger.log("workforce.runtime_start_failed", {
          rootDir: normalizedRootDir,
          errorMessage: error instanceof Error ? error.message : String(error),
        })
        throw error
      }
      runtimes.set(normalizedRootDir, runtime)
      return runtime.getWorkforce()
    },

    async getWorkforce(rootDir: string): Promise<WorkforceDescription> {
      const { runtime } = await getRuntime(rootDir)
      return runtime.getWorkforce()
    },

    async listWorkforces(): Promise<WorkforceStatus[]> {
      return Array.from(runtimes.values())
        .map((runtime) => runtime.getStatus())
        .sort((left, right) => left.rootDir.localeCompare(right.rootDir))
    },

    async shutdownWorkforce(rootDir: string): Promise<boolean> {
      const normalizedRootDir = await normalizeWorkforceRootDir(rootDir)
      const runtime = runtimes.get(normalizedRootDir)
      if (!runtime) {
        return false
      }

      await runtime.stop()
      runtimes.delete(normalizedRootDir)
      return true
    },

    async appendWorkforceEvent(
      rootDir: string,
      mutation: WorkforceManagerMutation,
      actor: WorkforceActorContext = {
        sessionId: null,
        rootDir: null,
        agentId: null,
        requestId: null,
      },
    ): Promise<{ workforce: WorkforceStatus; requestId: string | null }> {
      const { runtime } = await getRuntime(rootDir)
      let requestId: string | null = null

      switch (mutation.type) {
        case "request":
          requestId = await runtime.createRequest({
            targetAgentId: mutation.targetAgentId,
            payload: mutation.input,
            intent: mutation.intent,
            actor,
          })
          break
        case "update":
          await runtime.updateRequest({
            requestId: mutation.requestId,
            payload: mutation.input,
            actor,
          })
          requestId = mutation.requestId
          break
        case "cancel":
          await runtime.cancelRequest({
            requestId: mutation.requestId,
            reason: mutation.reason,
            actor,
          })
          requestId = mutation.requestId
          break
        case "truncate":
          await runtime.truncate({
            agentId: mutation.agentId,
            reason: mutation.reason,
            actor,
          })
          break
        case "respond":
          await runtime.respond({
            requestId: mutation.requestId,
            output: mutation.output,
            actor,
          })
          requestId = mutation.requestId
          break
        case "suspend":
          await runtime.suspend({
            requestId: mutation.requestId,
            reason: mutation.reason,
            actor,
          })
          requestId = mutation.requestId
          break
      }

      return {
        workforce: runtime.getStatus(),
        requestId,
      }
    },

    async close(): Promise<void> {
      for (const runtime of runtimes.values()) {
        await runtime.stop()
      }
      runtimes.clear()
    },
  }
}

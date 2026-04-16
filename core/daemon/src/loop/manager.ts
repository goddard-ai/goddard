import { IpcClientError } from "@goddard-ai/ipc"
import type { DaemonLoop, DaemonLoopStatus } from "@goddard-ai/schema/daemon"
import type { StartLoopRequest } from "@goddard-ai/schema/daemon/loops"

import { createLogger } from "../logging.ts"
import { resolveNamedLoopStartRequest, type ResolvedLoopStartRequest } from "../resolvers/loops.ts"
import { normalizeLoopIdentity } from "./paths.ts"
import { LoopRuntime, type LoopRuntimeDeps } from "./runtime.ts"

const logger = createLogger()

/** Optional lifecycle dependencies used to build new daemon-owned loop runtimes. */
export interface LoopManagerDeps extends LoopRuntimeDeps {
  createRuntime?: (input: ResolvedLoopStartRequest, deps: LoopRuntimeDeps) => Promise<LoopRuntime>
  resolveLoopStartRequest?: (input: StartLoopRequest) => Promise<ResolvedLoopStartRequest>
}

/** Daemon-owned loop runtime registry keyed by normalized repository root and loop name. */
export interface LoopManager {
  startLoop: (input: StartLoopRequest) => Promise<DaemonLoop>
  getLoop: (rootDir: string, loopName: string) => Promise<DaemonLoop>
  listLoops: () => Promise<DaemonLoopStatus[]>
  shutdownLoop: (rootDir: string, loopName: string) => Promise<boolean>
  close: () => Promise<void>
}

/** Creates the daemon loop manager that owns loop runtime lifecycle and lookup. */
export function createLoopManager(deps: LoopManagerDeps): LoopManager {
  const runtimes = new Map<string, LoopRuntime>()

  async function buildKey(rootDir: string, loopName: string): Promise<string> {
    const identity = await normalizeLoopIdentity(rootDir, loopName)
    return `${identity.rootDir}::${identity.loopName}`
  }

  return {
    async startLoop(input: StartLoopRequest): Promise<DaemonLoop> {
      const resolvedInput = await (deps.resolveLoopStartRequest ?? resolveNamedLoopStartRequest)(
        input,
      )
      const identity = await normalizeLoopIdentity(resolvedInput.rootDir, resolvedInput.loopName)
      const key = `${identity.rootDir}::${identity.loopName}`
      const existing = runtimes.get(key)
      if (existing) {
        logger.log("loop.runtime_reused", {
          rootDir: identity.rootDir,
          loopName: identity.loopName,
        })
        return existing.getLoop()
      }

      const runtime = await (deps.createRuntime ?? LoopRuntime.start)(
        {
          ...resolvedInput,
          rootDir: identity.rootDir,
          loopName: identity.loopName,
        },
        {
          sessionManager: deps.sessionManager,
          onStop: ({ rootDir, loopName }) => {
            void buildKey(rootDir, loopName).then((runtimeKey) => {
              runtimes.delete(runtimeKey)
            })
          },
        },
      )
      runtimes.set(key, runtime)
      return runtime.getLoop()
    },

    async getLoop(rootDir: string, loopName: string): Promise<DaemonLoop> {
      const runtime = runtimes.get(await buildKey(rootDir, loopName))
      if (!runtime) {
        throw new IpcClientError(`No loop is running for ${loopName} in ${rootDir}`)
      }

      return runtime.getLoop()
    },

    async listLoops(): Promise<DaemonLoopStatus[]> {
      return Array.from(runtimes.values())
        .map((runtime) => runtime.getStatus())
        .sort((left, right) =>
          left.rootDir === right.rootDir
            ? left.loopName.localeCompare(right.loopName)
            : left.rootDir.localeCompare(right.rootDir),
        )
    },

    async shutdownLoop(rootDir: string, loopName: string): Promise<boolean> {
      const key = await buildKey(rootDir, loopName)
      const runtime = runtimes.get(key)
      if (!runtime) {
        return false
      }

      runtimes.delete(key)
      await runtime.stop()
      return true
    },

    async close(): Promise<void> {
      for (const runtime of runtimes.values()) {
        await runtime.stop()
      }
      runtimes.clear()
    },
  }
}

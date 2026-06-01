import type { DbContext } from "@goddard-ai/daemon-plugin"
import { IpcClientError } from "@goddard-ai/ipc"

import type { DaemonLoop, DaemonLoopStatus, StartLoopRequest } from "../schema.ts"
import { normalizeLoopIdentity } from "./paths.ts"
import type { ResolvedLoopStartRequest } from "./resolver.ts"
import { LoopRuntime, type LoopRuntimeServices, type LoopRuntimeStartInput } from "./runtime.ts"
import type { loopDbSchema } from "./store.ts"

type LoopDb = DbContext<typeof loopDbSchema>

function isDaemonSessionId(value: string): value is `ses_${string}` {
  return value.startsWith("ses_")
}

/** Optional lifecycle dependencies used to build new daemon-owned loop runtimes. */
export interface LoopManagerInput extends LoopRuntimeServices {
  db: LoopDb
  createRuntime?: (input: LoopRuntimeStartInput) => Promise<LoopRuntime>
  resolveLoopStartRequest: (input: StartLoopRequest) => Promise<ResolvedLoopStartRequest>
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
export function createLoopManager(input: LoopManagerInput): LoopManager {
  const logger = input.log.createLogger()
  const runtimes = new Map<string, LoopRuntime>()

  async function buildKey(rootDir: string, loopName: string): Promise<string> {
    const identity = await normalizeLoopIdentity(rootDir, loopName)
    return `${identity.rootDir}::${identity.loopName}`
  }

  return {
    async startLoop(request: StartLoopRequest): Promise<DaemonLoop> {
      const resolvedInput = await input.resolveLoopStartRequest(request)
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

      const runtime = await (input.createRuntime ?? LoopRuntime.start)({
        config: {
          ...resolvedInput,
          rootDir: identity.rootDir,
          loopName: identity.loopName,
        },
        log: input.log,
        session: input.session,
        onStop: ({ rootDir, loopName }) => {
          void buildKey(rootDir, loopName).then((runtimeKey) => {
            runtimes.delete(runtimeKey)
          })
        },
      })
      runtimes.set(key, runtime)
      const loop = runtime.getLoop()
      if (!isDaemonSessionId(loop.sessionId)) {
        throw new Error(`Loop runtime returned invalid daemon session id: ${loop.sessionId}`)
      }
      const existingRecord =
        input.db.loopSessions.first({
          where: { sessionId: loop.sessionId },
        }) ?? null
      const nextRecord = {
        sessionId: loop.sessionId,
        rootDir: loop.rootDir,
        loopName: loop.loopName,
        promptModulePath: loop.promptModulePath,
      }
      if (existingRecord) {
        input.db.loopSessions.put(existingRecord.id, nextRecord)
      } else {
        input.db.loopSessions.create(nextRecord)
      }
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

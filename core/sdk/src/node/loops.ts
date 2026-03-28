import { resolve } from "node:path"
import type { AgentLoopRuntimeOverrides, LoopClientOptions } from "../daemon/loops.ts"
import {
  getDaemonLoop,
  listDaemonLoops,
  shutdownDaemonLoop,
  startDaemonLoop,
} from "../daemon/loops.ts"
import { type NodeDaemonClientOptions, resolveNodeDaemonClient } from "./client.ts"

/** Starts a new named background loop managed by the daemon using Node cwd defaults. */
export async function startNamedLoop(
  loopName: string,
  overrides?: AgentLoopRuntimeOverrides,
  options?: NodeDaemonClientOptions,
) {
  return startDaemonLoop(
    {
      rootDir: resolve(overrides?.session?.cwd ?? process.cwd()),
      loopName,
      session: overrides?.session,
      rateLimits: overrides?.rateLimits,
      retries: overrides?.retries,
    },
    { client: resolveNodeDaemonClient(options) },
  )
}

/** Fetches one daemon-owned loop runtime for the given repository root and loop name. */
export async function getLoop(
  rootDir: string,
  loopName: string,
  options?: NodeDaemonClientOptions,
) {
  return getDaemonLoop(resolve(rootDir), loopName, {
    client: resolveNodeDaemonClient(options),
  })
}

/** Lists all daemon-owned loop runtimes currently running in the daemon. */
export async function listLoops(options?: NodeDaemonClientOptions) {
  return listDaemonLoops({ client: resolveNodeDaemonClient(options) })
}

/** Stops one daemon-owned loop runtime for the given repository root and loop name. */
export async function stopLoop(
  rootDir: string,
  loopName: string,
  options?: NodeDaemonClientOptions,
) {
  return shutdownDaemonLoop(resolve(rootDir), loopName, {
    client: resolveNodeDaemonClient(options),
  })
}

export type { AgentLoopRuntimeOverrides, LoopClientOptions }

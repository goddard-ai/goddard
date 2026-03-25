import type {
  DaemonLoop,
  DaemonLoopStatus,
  StartDaemonLoopRequest,
} from "@goddard-ai/schema/daemon"
import { resolve } from "node:path"
import { resolveDaemonClient, type DaemonClientOptions } from "./client.ts"

/** Shared daemon connection options for loop lifecycle helpers. */
export type LoopClientOptions = DaemonClientOptions

/** Runtime overrides accepted when starting one packaged daemon-owned loop. */
export type AgentLoopRuntimeOverrides = Pick<
  StartDaemonLoopRequest,
  "session" | "rateLimits" | "retries"
>

/** Starts or reuses one daemon-managed loop runtime. */
export async function startDaemonLoop(
  input: StartDaemonLoopRequest,
  options?: LoopClientOptions,
): Promise<DaemonLoop> {
  const client = resolveDaemonClient(options)
  const response = await client.send("loopStart", input)
  return response.loop
}

/** Returns the daemon-managed loop state for one repository root and loop name. */
export async function getDaemonLoop(
  rootDir: string,
  loopName: string,
  options?: LoopClientOptions,
): Promise<DaemonLoop> {
  const client = resolveDaemonClient(options)
  const response = await client.send("loopGet", { rootDir, loopName })
  return response.loop
}

/** Lists all daemon-managed loop runtimes currently known to the daemon. */
export async function listDaemonLoops(options?: LoopClientOptions): Promise<DaemonLoopStatus[]> {
  const client = resolveDaemonClient(options)
  const response = await client.send("loopList", {})
  return response.loops
}

/** Shuts down one daemon-managed loop runtime. */
export async function shutdownDaemonLoop(
  rootDir: string,
  loopName: string,
  options?: LoopClientOptions,
): Promise<boolean> {
  const client = resolveDaemonClient(options)
  const response = await client.send("loopShutdown", { rootDir, loopName })
  return response.success
}

/** Starts a new named background loop managed by the daemon. */
export async function startNamedLoop(
  loopName: string,
  overrides?: AgentLoopRuntimeOverrides,
  options?: LoopClientOptions,
): Promise<DaemonLoop> {
  return startDaemonLoop(
    {
      rootDir: resolve(overrides?.session?.cwd ?? process.cwd()),
      loopName,
      session: overrides?.session,
      rateLimits: overrides?.rateLimits,
      retries: overrides?.retries,
    },
    options,
  )
}

/** Fetches one daemon-owned loop runtime for the given repository root and loop name. */
export async function getLoop(
  rootDir: string,
  loopName: string,
  options?: LoopClientOptions,
): Promise<DaemonLoop> {
  return getDaemonLoop(resolve(rootDir), loopName, options)
}

/** Lists all daemon-owned loop runtimes currently running in the daemon. */
export async function listLoops(options?: LoopClientOptions): Promise<DaemonLoopStatus[]> {
  return listDaemonLoops(options)
}

/** Stops one daemon-owned loop runtime for the given repository root and loop name. */
export async function stopLoop(
  rootDir: string,
  loopName: string,
  options?: LoopClientOptions,
): Promise<boolean> {
  return shutdownDaemonLoop(resolve(rootDir), loopName, options)
}

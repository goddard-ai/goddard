import type {
  DaemonLoop,
  DaemonLoopStatus,
  StartDaemonLoopRequest,
} from "@goddard-ai/schema/daemon"
import { resolveDaemonClient, type DaemonClientOptions } from "./client.js"

/** Shared daemon connection options for loop lifecycle helpers. */
export type LoopClientOptions = DaemonClientOptions

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

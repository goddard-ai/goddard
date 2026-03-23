import type { DaemonLoop, DaemonLoopStatus } from "@goddard-ai/schema/daemon"
import type { StartDaemonLoopRequest } from "@goddard-ai/schema/daemon/loops"
import { LoopRuntime, type LoopRuntimeDeps } from "./runtime.ts"
/** Optional lifecycle dependencies used to build new daemon-owned loop runtimes. */
export interface LoopManagerDeps extends LoopRuntimeDeps {
  createRuntime?: (input: StartDaemonLoopRequest, deps: LoopRuntimeDeps) => Promise<LoopRuntime>
}
/** Daemon-owned loop runtime registry keyed by normalized repository root and loop name. */
export interface LoopManager {
  startLoop: (input: StartDaemonLoopRequest) => Promise<DaemonLoop>
  getLoop: (rootDir: string, loopName: string) => Promise<DaemonLoop>
  listLoops: () => Promise<DaemonLoopStatus[]>
  shutdownLoop: (rootDir: string, loopName: string) => Promise<boolean>
  close: () => Promise<void>
}
/** Creates the daemon loop manager that owns loop runtime lifecycle and lookup. */
export declare function createLoopManager(deps: LoopManagerDeps): LoopManager
//# sourceMappingURL=manager.d.ts.map

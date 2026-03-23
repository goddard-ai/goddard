import type { DaemonLoop, DaemonLoopStatus } from "@goddard-ai/schema/daemon"
import type { StartDaemonLoopRequest } from "@goddard-ai/schema/daemon/loops"
import type { SessionManager } from "../session/index.ts"
/** Runtime dependencies shared by one daemon-owned loop host. */
export interface LoopRuntimeDeps {
  sessionManager: SessionManager
  onStop?: (input: { rootDir: string; loopName: string }) => void
}
/** Daemon-owned loop runtime backed by one persistent daemon session. */
export declare class LoopRuntime {
  #private
  private constructor()
  /** Starts one daemon-owned loop runtime and begins background cycle execution. */
  static start(config: StartDaemonLoopRequest, deps: LoopRuntimeDeps): Promise<LoopRuntime>
  /** Returns the full daemon loop record exposed by start and get calls. */
  getLoop(): DaemonLoop
  /** Returns the current public runtime status for one daemon-owned loop. */
  getStatus(): DaemonLoopStatus
  /** Stops the loop runtime and shuts down its backing daemon session. */
  stop(): Promise<void>
}
//# sourceMappingURL=runtime.d.ts.map

import type { DaemonWorkforce, DaemonWorkforceStatus } from "@goddard-ai/schema/daemon"
import type {
  WorkforceAgentConfig,
  WorkforceConfig,
  WorkforceLedgerEvent,
  WorkforceRequestIntent,
  WorkforceRequestRecord,
} from "@goddard-ai/schema/workforce"
import type { SessionManager } from "../session/index.ts"
/** Optional authenticated agent context derived from the calling daemon session. */
export interface WorkforceActorContext {
  sessionId: string | null
  agentId: string | null
  requestId: string | null
}
/** Input delivered to the daemon-owned session runner for one handled request. */
export interface WorkforceSessionRunInput {
  rootDir: string
  agent: WorkforceAgentConfig
  config: WorkforceConfig
  request: WorkforceRequestRecord
  recentActivity: WorkforceLedgerEvent[]
}
/** Session runner abstraction used by tests and the real daemon session bridge. */
export type WorkforceSessionRunner = (input: WorkforceSessionRunInput) => Promise<void>
/** Mutable runtime dependencies shared by one workload host. */
export interface WorkforceRuntimeDeps {
  sessionManager: SessionManager
  runSession?: WorkforceSessionRunner
}
/** A daemon-managed repo-local workforce runtime and its active queue state. */
export declare class WorkforceRuntime {
  #private
  private constructor()
  /** Rehydrates one repository workforce and resumes draining any queued requests. */
  static start(rootDir: string, deps: WorkforceRuntimeDeps): Promise<WorkforceRuntime>
  getWorkforce(): DaemonWorkforce
  getStatus(): DaemonWorkforceStatus
  stop(): Promise<void>
  createRequest(input: {
    targetAgentId: string
    payload: string
    intent?: WorkforceRequestIntent
    actor: WorkforceActorContext
  }): Promise<string>
  updateRequest(input: {
    requestId: string
    payload: string
    actor: WorkforceActorContext
  }): Promise<void>
  cancelRequest(input: {
    requestId: string
    reason: string | null
    actor: WorkforceActorContext
  }): Promise<void>
  truncate(input: {
    agentId: string | null
    reason: string | null
    actor: WorkforceActorContext
  }): Promise<void>
  respond(input: { requestId: string; output: string; actor: WorkforceActorContext }): Promise<void>
  suspend(input: { requestId: string; reason: string; actor: WorkforceActorContext }): Promise<void>
  private appendEvent
  private scheduleDrainForAllAgents
  private scheduleDrain
  private drainAgent
  private processRequest
  private handleAttemptFailure
}
//# sourceMappingURL=runtime.d.ts.map

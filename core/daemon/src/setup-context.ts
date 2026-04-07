/** Daemon-scoped async-context variables used during setup and runtime log correlation. */
import { AsyncContext } from "@b9g/async-context"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import type { ConfigManager } from "./config-manager.ts"
import type { FeedbackEvent } from "./feedback.ts"

/** Setup-only dependencies installed while daemon construction is running. */
export type DaemonSetupContext = {
  runtime: {
    baseUrl: string
    socketPath: string
    agentBinDir: string
  }
  configManager: ConfigManager
}

/** Mutable IPC request context shared across one daemon server request lifecycle. */
export type DaemonIpcRequestContext = {
  opId: string
  sessionId: DaemonSession["id"] | undefined
  setSessionId: (sessionId: DaemonSession["id"]) => void
}

/** Stable session metadata carried through live daemon session work. */
export type DaemonSessionContext = {
  sessionId: DaemonSession["id"]
  acpSessionId?: string
  cwd: string
  repository?: string
  prNumber?: number
  worktreeDir?: string
  worktreePoweredBy?: string
}

/** Authenticated workforce actor identity attached to one mutation call. */
export type DaemonWorkforceActorContext = {
  actorSessionId?: string
  actorAgentId?: string
  actorRequestId?: string
}

/** Active workforce dispatch metadata carried while one request attempt is running. */
export type DaemonWorkforceDispatchContext = {
  rootDir: string
  agentId: string
  requestId: string
  attempt: number
}

/** Active loop runtime identity carried while one loop is executing work. */
export type DaemonLoopContext = {
  rootDir: string
  loopName: string
  sessionId: DaemonSession["id"]
  acpSessionId: string
}

/** Repository feedback metadata carried while one background feedback event is handled. */
export type DaemonFeedbackEventContext = {
  repository: string
  prNumber: number
  feedbackType: FeedbackEvent["type"]
}

export const daemonSetupContext = new AsyncContext.Variable<DaemonSetupContext>()
export const daemonIpcRequestContext = new AsyncContext.Variable<DaemonIpcRequestContext>()
export const daemonSessionContext = new AsyncContext.Variable<DaemonSessionContext>()
export const daemonWorkforceActorContext = new AsyncContext.Variable<DaemonWorkforceActorContext>()
export const daemonWorkforceDispatchContext =
  new AsyncContext.Variable<DaemonWorkforceDispatchContext>()
export const daemonLoopContext = new AsyncContext.Variable<DaemonLoopContext>()
export const daemonFeedbackEventContext = new AsyncContext.Variable<DaemonFeedbackEventContext>()

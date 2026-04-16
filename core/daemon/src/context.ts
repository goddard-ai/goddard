/** Daemon-scoped async-context variables used during setup and runtime log correlation. */
import { AsyncContext } from "@b9g/async-context"
import type { DaemonSession } from "@goddard-ai/schema/daemon"

import type { ConfigManager } from "./config-manager.ts"
import type { FeedbackEvent } from "./feedback.ts"

/** Setup-only dependencies installed while daemon construction is running. */
export type SetupContext = {
  runtime: {
    baseUrl: string
    socketPath: string
    agentBinDir: string
  }
  configManager: ConfigManager
}

/** Mutable IPC request context shared across one daemon server request lifecycle. */
export type IpcRequestContext = {
  opId: string
  sessionId: DaemonSession["id"] | null
  setSessionId: (sessionId: DaemonSession["id"]) => void
}

/** Stable session metadata carried through live daemon session work. */
export type SessionContext = {
  sessionId: DaemonSession["id"]
  acpSessionId: string | null
  cwd: string
  repository: string | null
  prNumber: number | null
  worktreeDir: string | null
  worktreePoweredBy: string | null
}

/** Authenticated workforce actor identity attached to one mutation call. */
export type WorkforceActorContext = {
  sessionId: string | null
  rootDir: string | null
  agentId: string | null
  requestId: string | null
}

/** Active workforce dispatch metadata carried while one request attempt is running. */
export type WorkforceDispatchContext = {
  rootDir: string
  agentId: string
  requestId: string
  attempt: number
}

/** Active loop runtime identity carried while one loop is executing work. */
export type LoopContext = {
  rootDir: string
  loopName: string
  sessionId: DaemonSession["id"]
  acpSessionId: string
}

/** Repository feedback metadata carried while one background feedback event is handled. */
export type FeedbackEventContext = {
  repository: string
  prNumber: number
  feedbackType: FeedbackEvent["type"]
}

export const SetupContext = new AsyncContext.Variable<SetupContext>()
export const IpcRequestContext = new AsyncContext.Variable<IpcRequestContext>()
export const SessionContext = new AsyncContext.Variable<SessionContext>()
export const WorkforceActorContext = new AsyncContext.Variable<WorkforceActorContext>()
export const WorkforceDispatchContext = new AsyncContext.Variable<WorkforceDispatchContext>()
export const LoopContext = new AsyncContext.Variable<LoopContext>()
export const FeedbackEventContext = new AsyncContext.Variable<FeedbackEventContext>()

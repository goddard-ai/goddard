/** Daemon-scoped async-context variables used during setup and runtime log correlation. */
import { AsyncContext } from "@b9g/async-context"
import type { DaemonSession } from "@goddard-ai/schema/daemon"

import type { ConfigManager } from "./config-manager.ts"
import type { FeedbackEvent } from "./feedback.ts"

/** Setup-only dependencies installed while daemon construction is running. */
export type SetupContext = {
  runtime: {
    baseUrl: string
    port: number
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

/** Repository feedback metadata carried while one background feedback event is handled. */
export type FeedbackEventContext = {
  repository: string
  prNumber: number
  feedbackType: FeedbackEvent["type"]
}

export const SetupContext = new AsyncContext.Variable<SetupContext>()
export const IpcRequestContext = new AsyncContext.Variable<IpcRequestContext>()
export const SessionContext = new AsyncContext.Variable<SessionContext>()
export const FeedbackEventContext = new AsyncContext.Variable<FeedbackEventContext>()

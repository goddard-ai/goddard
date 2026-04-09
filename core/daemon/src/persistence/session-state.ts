import type { DaemonSession, DaemonSessionDiagnosticEvent } from "@goddard-ai/schema/daemon/store"

/** Durable connectivity summary for a daemon session across daemon restarts. */
export type SessionConnectionMode = DaemonSession["connectionMode"]

/** Structured session diagnostic event persisted for postmortem inspection. */
export type SessionDiagnosticEvent = DaemonSessionDiagnosticEvent

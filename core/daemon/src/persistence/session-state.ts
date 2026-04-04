/** Durable connectivity summary for a daemon session across daemon restarts. */
export type SessionConnectionMode = "live" | "history" | "none"

/** Structured session diagnostic event persisted for postmortem inspection. */
export type SessionDiagnosticEvent = {
  type: string
  at: string
  detail?: Record<string, unknown>
}

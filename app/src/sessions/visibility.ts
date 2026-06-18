import type { DaemonSession } from "@goddard-ai/sdk"

export function filterPrimarySessions(sessions: readonly DaemonSession[]) {
  return sessions.filter((session) => session.visibility === "visible" && !session.completedHidden)
}

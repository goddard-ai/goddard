import { kind } from "kindstore"

import {
  DaemonSession,
  DaemonSessionDiagnostics,
  DaemonSessionTurn,
  DaemonSessionTurnDraft,
  DaemonWorktree,
} from "../schema.ts"

/** Session-owned kindstore collections contributed to the daemon composition root. */
export const sessionDbSchema = {
  sessions: kind("ses", DaemonSession)
    .createdAt()
    .updatedAt()
    .index("acpSessionId")
    .index("repository")
    .index("token")
    .multi("repository_prNumber", {
      repository: "asc",
      prNumber: "asc",
    })
    .multi("updatedAt_id", {
      updatedAt: "desc",
      id: "desc",
    })
    .multi("completedHidden_updatedAt_id", {
      completedHidden: "asc",
      updatedAt: "desc",
      id: "desc",
    }),

  sessionTurns: kind("trn", DaemonSessionTurn)
    .index("sessionId", { type: "text" })
    .index("sequence", { type: "integer" })
    .multi("sessionId_sequence", {
      sessionId: "asc",
      sequence: "desc",
    }),

  sessionTurnDrafts: kind("drf", DaemonSessionTurnDraft)
    .index("sessionId", { type: "text" })
    .index("sequence", { type: "integer" })
    .multi("sessionId_sequence", {
      sessionId: "asc",
      sequence: "desc",
    }),

  sessionDiagnostics: kind("dgn", DaemonSessionDiagnostics).index("sessionId", {
    type: "text",
  }),

  worktrees: kind("wt", DaemonWorktree).index("sessionId", { type: "text" }),
}

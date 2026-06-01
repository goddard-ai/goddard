import { kind } from "kindstore"

import { DaemonLoopSession } from "../schema.ts"

/** Daemon persistence owned by the loop feature. */
export const loopDbSchema = {
  loopSessions: kind("lop", DaemonLoopSession)
    .index("sessionId", { type: "text" })
    .multi("rootDir_loopName", {
      rootDir: "asc",
      loopName: "asc",
    }),
}

import { kind } from "kindstore"

import { DaemonWorkforce } from "../schema.ts"

/** Daemon persistence owned by the workforce feature. */
export const workforceDbSchema = {
  workforces: kind("wf", DaemonWorkforce).index("sessionId", { type: "text" }),
}

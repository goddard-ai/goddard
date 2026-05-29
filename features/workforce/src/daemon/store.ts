import { DaemonWorkforce } from "@goddard-ai/schema/daemon/store"
import { kind } from "kindstore"

/** Daemon persistence owned by the workforce feature. */
export const workforceDbSchema = {
  workforces: kind("wf", DaemonWorkforce).index("sessionId", { type: "text" }),
}

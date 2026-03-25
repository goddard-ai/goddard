import { InlineSessionParams } from "../config.ts"
import { z } from "zod"

/** Request payload used to run one named daemon-resolved action. */
export const RunNamedDaemonActionRequest = InlineSessionParams.extend({
  actionName: z.string().min(1),
  cwd: z.string().min(1),
})

/** Type shape for one named action execution request routed over daemon IPC. */
export type RunNamedDaemonActionRequest = z.infer<typeof RunNamedDaemonActionRequest>

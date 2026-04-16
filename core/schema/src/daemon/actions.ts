import { z } from "zod"

import { InlineSessionParams } from "../config.ts"

/** Request payload used to run one named daemon-resolved action. */
export const RunNamedActionRequest = InlineSessionParams.extend({
  actionName: z.string().min(1),
  cwd: z.string().min(1),
})

/** Type shape for one named action execution request routed over daemon IPC. */
export type RunNamedActionRequest = z.infer<typeof RunNamedActionRequest>

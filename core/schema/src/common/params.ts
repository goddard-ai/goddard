import { z } from "zod"

/** Stable path and payload params used to address one daemon session by id. */
export const DaemonSessionIdParams = z.object({
  id: z.string(),
})

export type DaemonSessionIdParams = z.infer<typeof DaemonSessionIdParams>

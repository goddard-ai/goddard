import { z } from "zod"

/** Tagged daemon session id emitted by the daemon session store. */
export const DaemonSessionId = z.custom<`ses_${string}`>(
  (value): value is `ses_${string}` => typeof value === "string" && value.startsWith("ses_"),
)

export type DaemonSessionId = z.infer<typeof DaemonSessionId>

/** Stable path and payload params used to address one daemon session by id. */
export const DaemonSessionIdParams = z.object({
  id: DaemonSessionId,
})

export type DaemonSessionIdParams = z.infer<typeof DaemonSessionIdParams>

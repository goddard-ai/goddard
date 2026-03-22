import { z } from "zod"

/** Free-form daemon session metadata shared by session creation contracts. */
export const DaemonSessionMetadata = z
  .object({})
  .catchall(z.unknown())
  .describe("Free-form metadata attached to the daemon session for downstream consumers.")

export type DaemonSessionMetadata = z.infer<typeof DaemonSessionMetadata>

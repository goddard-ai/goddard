import { z } from "zod"

/** Free-form daemon session metadata shared by session creation contracts. */
export const DaemonSessionMetadata = z
  .object({
    repository: z.string().optional(),
    prNumber: z.number().int().optional(),
  })
  .catchall(z.unknown())

export type DaemonSessionMetadata = z.infer<typeof DaemonSessionMetadata>

import { z } from "zod"

/** Standard authenticated HTTP headers shared by backend and daemon routes. */
export const BearerHeaders = z.object({
  authorization: z.string(),
})

export type BearerHeaders = z.infer<typeof BearerHeaders>

import { z } from "zod"

/** Repository identity for a remotely hosted source repository. */
export const RemoteRepositoryRef = z.object({
  owner: z.string(),
  name: z.string(),
})

export type RemoteRepositoryRef = z.infer<typeof RemoteRepositoryRef>

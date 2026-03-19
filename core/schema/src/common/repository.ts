import { z } from "zod"

/** Repository owner and name tuple used by GitHub-backed contracts. */
export const RepoRef = z.object({
  owner: z.string(),
  repo: z.string(),
})

export type RepoRef = z.infer<typeof RepoRef>

/** Repository and pull request number tuple shared by PR-scoped contracts. */
export const RepoPrRef = RepoRef.extend({
  prNumber: z.number(),
})

export type RepoPrRef = z.infer<typeof RepoPrRef>

import { z } from "zod"

/** Persisted worktree identity attached to daemon session metadata. */
export const SessionWorktreeMetadata = z
  .strictObject({
    repoRoot: z.string(),
    requestedCwd: z.string(),
    effectiveCwd: z.string(),
    worktreeDir: z.string(),
    branchName: z.string(),
    poweredBy: z.string(),
  })
  .describe("Persisted identity for one daemon-managed worktree.")

export type SessionWorktreeMetadata = z.infer<typeof SessionWorktreeMetadata>

/** Free-form daemon session metadata shared by session creation contracts. */
export const DaemonSessionMetadata = z
  .object({
    worktree: SessionWorktreeMetadata.optional(),
  })
  .catchall(z.unknown())
  .describe("Free-form metadata attached to the daemon session for downstream consumers.")

export type DaemonSessionMetadata = z.infer<typeof DaemonSessionMetadata>

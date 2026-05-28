import { DaemonSessionId, DaemonSessionIdParams } from "@goddard-ai/schema/id"
import { z } from "zod"

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.strictObject({
  enabled: z.boolean().optional(),
  baseBranchName: z.string().optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Response payload fragment returned after one daemon-managed session worktree fetch. */
export const SessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
})

export type SessionWorktree = z.infer<typeof SessionWorktree>

/** Session identity fragment shared by worktree responses. */
export type SessionWorktreeIdentity = {
  id: DaemonSessionId
  acpSessionId: string
}

/** Response payload returned after one daemon-managed session worktree fetch. */
export type GetSessionWorktreeResponse = SessionWorktreeIdentity & {
  worktree: SessionWorktree | null
}

/** Request payload used to read one daemon-managed session worktree. */
export const GetSessionWorktreeRequest = DaemonSessionIdParams

export type GetSessionWorktreeRequest = z.infer<typeof GetSessionWorktreeRequest>

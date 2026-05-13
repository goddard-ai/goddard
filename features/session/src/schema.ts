import { DaemonSessionId, DaemonSessionIdParams } from "@goddard-ai/schema/common/params"
import { z } from "zod"

/** Review-session options accepted by the daemon session API. */
export const SessionReviewSessionParams = z.strictObject({
  enabled: z.boolean().optional(),
})

export type SessionReviewSessionParams = z.infer<typeof SessionReviewSessionParams>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.strictObject({
  enabled: z.boolean().optional(),
  baseBranchName: z.string().optional(),
  reviewSession: SessionReviewSessionParams.optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Live review-session state returned exactly as the review session engine reports it. */
export const SessionReviewSessionState = z.strictObject({
  sessionId: z.string(),
  agentWorktree: z.string(),
  reviewWorktree: z.string(),
  agentBranch: z.string(),
  reviewBranch: z.string(),
  paused: z.boolean(),
  refs: z.strictObject({
    agentSnapshot: z.string(),
    renderedSnapshot: z.string(),
  }),
  agentSnapshot: z.string().nullable(),
  renderedSnapshot: z.string().nullable(),
  lastSync: z.strictObject({
    status: z.union([
      z.literal("synced"),
      z.literal("rejected-human-patch"),
      z.literal("paused"),
      z.literal("error"),
    ]),
    acceptedPatch: z.string().nullable(),
    rejectedPatch: z.string().nullable(),
  }),
  patchCounts: z.strictObject({
    accepted: z.number().int(),
    rejected: z.number().int(),
  }),
})

export type SessionReviewSessionState = z.infer<typeof SessionReviewSessionState>

/** Response payload fragment returned after one daemon-managed session worktree fetch. */
export const SessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
  reviewSession: SessionReviewSessionState.nullable(),
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

/** Request payload used to mount a review session for one daemon-managed session worktree. */
export const MountSessionReviewSessionRequest = DaemonSessionIdParams

export type MountSessionReviewSessionRequest = z.infer<typeof MountSessionReviewSessionRequest>

/** Request payload used to run one mounted review session immediately. */
export const RunSessionReviewSessionRequest = DaemonSessionIdParams

export type RunSessionReviewSessionRequest = z.infer<typeof RunSessionReviewSessionRequest>

/** Request payload used to unmount a review session from one daemon-managed session worktree. */
export const UnmountSessionReviewSessionRequest = DaemonSessionIdParams

export type UnmountSessionReviewSessionRequest = z.infer<typeof UnmountSessionReviewSessionRequest>

/** Response payload returned after one daemon-managed review session mutation. */
export type MutateSessionReviewSessionResponse = SessionWorktreeIdentity & {
  worktree: SessionWorktree | null
  warnings: string[]
}

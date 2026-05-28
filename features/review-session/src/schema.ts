import { DaemonSessionIdParams } from "@goddard-ai/schema/id"
import type { SessionWorktree } from "@goddard-ai/session/schema"
import { z } from "zod"

/** Review-session options accepted by daemon session launch worktrees. */
export const ReviewSessionLaunchParams = z.strictObject({
  enabled: z.boolean().optional(),
})

export type ReviewSessionLaunchParams = z.infer<typeof ReviewSessionLaunchParams>

/** Live review-session state returned exactly as the review session engine reports it. */
export const ReviewSessionState = z.strictObject({
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

export type ReviewSessionState = z.infer<typeof ReviewSessionState>

/** Request payload used to read one daemon-managed review session. */
export const GetReviewSessionRequest = DaemonSessionIdParams

export type GetReviewSessionRequest = z.infer<typeof GetReviewSessionRequest>

/** Request payload used to mount a review session for one daemon-managed session worktree. */
export const MountReviewSessionRequest = DaemonSessionIdParams

export type MountReviewSessionRequest = z.infer<typeof MountReviewSessionRequest>

/** Request payload used to run one mounted review session immediately. */
export const RunReviewSessionRequest = DaemonSessionIdParams

export type RunReviewSessionRequest = z.infer<typeof RunReviewSessionRequest>

/** Request payload used to unmount a review session from one daemon-managed session worktree. */
export const UnmountReviewSessionRequest = DaemonSessionIdParams

export type UnmountReviewSessionRequest = z.infer<typeof UnmountReviewSessionRequest>

/** Response payload returned after reading or mutating one daemon-managed review session. */
export type ReviewSessionResponse = {
  id: GetReviewSessionRequest["id"]
  acpSessionId: string
  worktree: SessionWorktree | null
  reviewSession: ReviewSessionState | null
  warnings: string[]
}

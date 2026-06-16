/** Review-sync command names reported by CLI and TypeScript command functions. */
import type { GitHost } from "./git.ts"

export type ReviewSyncCommand =
  | "start"
  | "sync"
  | "status"
  | "pause"
  | "resume"
  | "cleanup"
  | "watch"
  | "stop"

/** Stable top-level status values returned to callers. */
export type ReviewSyncStatus = "ok" | "rejected-human-patch" | "paused" | "error"

/** Watch phases where callers may need to delay a mutating refresh or sync. */
export type WatchSyncReadyReason = "start" | "sync" | "branch-ref-refresh"

/** Last-sync statuses stored in durable session state. */
export type LastSyncStatus = "synced" | "rejected-human-patch" | "paused" | "error"

/** Structured status payload returned by statusReviewSession for TypeScript callers. */
export type ReviewSyncStatusData = {
  sessionId: string
  agentWorktree: string
  reviewWorktree: string
  agentBranch: string
  reviewBranch: string
  paused: boolean
  refs: {
    agentSnapshot: string
    renderedSnapshot: string
  }
  agentSnapshot: string | null
  renderedSnapshot: string | null
  lastSync: {
    status: LastSyncStatus
    acceptedPatch: string | null
    rejectedPatch: string | null
  }
  patchCounts: {
    accepted: number
    rejected: number
  }
}

/** Structured command result returned by successful operations and CLI-safe handlers. */
export type ReviewSyncResult = {
  exitCode: number
  command: ReviewSyncCommand
  status: ReviewSyncStatus
  sessionId?: string
  reviewBranch?: string
  acceptedPatchPath?: string
  rejectedPatchPath?: string
  verbose?: boolean
  data?: ReviewSyncStatusData
  message: string
}

/** Worktree location shared by commands that infer their active session. */
export type ReviewSyncWorktreeInput = {
  cwd: string
}

/** Inputs for creating or reusing a review-sync session. */
export type StartReviewSyncInput = ReviewSyncWorktreeInput & {
  agentBranch: string
}

/** Inputs for reading review-sync session state. */
export type StatusReviewSyncInput = ReviewSyncWorktreeInput & {
  json?: boolean
}

/** Inputs for listing review-sync sessions in one Git repository. */
export type ListReviewSyncInput = ReviewSyncWorktreeInput

/** Inputs for removing saved review-sync sessions that match one worktree. */
export type CleanupReviewSyncInput = ReviewSyncWorktreeInput & {
  all?: boolean
}

/** Inputs for watching a review-sync session until the caller aborts it. */
export type WatchReviewSyncInput = ReviewSyncWorktreeInput & {
  agentBranch?: string
  signal?: AbortSignal
  verbose?: boolean
  onResult?: (result: ReviewSyncResult) => void | Promise<void>
  waitForSyncReady?: (reason: WatchSyncReadyReason) => boolean | void | Promise<boolean | void>
}

/** Normalized runtime context passed through internal operations. */
export type RuntimeContext = {
  cwd: string
  gitHost: GitHost
}

/** Durable session state stored under the Git common directory. */
export type SessionState = {
  schemaVersion: 1
  sessionId: string
  repoCommonDir: string
  agentWorktree: string
  reviewWorktree: string
  agentBranch: string
  reviewBranch: string
  refs: {
    agentSnapshot: string
    renderedSnapshot: string
  }
  paused: boolean
  createdAt: string
  updatedAt: string
  lastSync: {
    status: LastSyncStatus
    acceptedPatch: string | null
    rejectedPatch: string | null
  }
}

/** Result of applying or rejecting the current human review patch. */
export type PatchFlowResult = {
  status: "synced" | "rejected-human-patch"
  acceptedPatchPath: string | null
  rejectedPatchPath: string | null
}

export const schemaVersion = 1
export const reviewBranchPrefix = "review-sync/"
export const lockStaleAfterMs = 10 * 60 * 1000

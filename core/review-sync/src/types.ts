/** Review-sync command names supported by the CLI and programmatic entrypoint. */
export type ReviewSyncCommand = "start" | "sync" | "status" | "pause" | "resume" | "watch"

/** Stable top-level status values returned to callers. */
export type ReviewSyncStatus = "ok" | "rejected-human-patch" | "paused" | "error"

/** Last-sync statuses stored in durable session state. */
export type LastSyncStatus = "synced" | "rejected-human-patch" | "paused" | "error"

/** Structured command result returned by both CLI and embedded callers. */
export type ReviewSyncResult = {
  exitCode: number
  command: ReviewSyncCommand
  status: ReviewSyncStatus
  sessionId?: string
  reviewBranch?: string
  acceptedPatchPath?: string
  rejectedPatchPath?: string
  message: string
}

/** Worktree location shared by commands that infer their active session. */
export type ReviewSyncWorktreeInput = {
  cwd: string
}

/** Inputs for creating or reusing a review-sync session. */
export type StartReviewSyncInput = ReviewSyncWorktreeInput & {
  reviewWorktree: string
}

/** Inputs for reading review-sync session state. */
export type StatusReviewSyncInput = ReviewSyncWorktreeInput & {
  json?: boolean
}

/** Inputs for watching a review-sync session until the caller aborts it. */
export type WatchReviewSyncInput = ReviewSyncWorktreeInput & {
  intervalMs?: number
  signal?: AbortSignal
  onResult?: (result: ReviewSyncResult) => void | Promise<void>
}

/** Normalized runtime context passed through internal operations. */
export type RuntimeContext = {
  cwd: string
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

/** Parsed Git worktree-list entry used for branch ownership validation. */
export type WorktreeInfo = {
  path: string
  branch: string | null
}

/** Captured subprocess result used by Git command wrappers. */
export type CommandResult = {
  status: number
  stdout: string
  stderr: string
}

/** Result of applying or rejecting the current human review patch. */
export type PatchFlowResult = {
  status: "synced" | "rejected-human-patch"
  acceptedPatchPath: string | null
  rejectedPatchPath: string | null
}

export const schemaVersion = 1
export const reviewBranchSuffix = "--review"
export const lockStaleAfterMs = 10 * 60 * 1000

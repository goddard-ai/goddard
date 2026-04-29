/** Review-sync command names supported by the CLI and programmatic entrypoint. */
export type ReviewSyncCommand = "start" | "sync" | "status" | "pause" | "resume"

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

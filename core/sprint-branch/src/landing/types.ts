import type { SprintBranchState, SprintDiagnostic } from "../types"

/** Shared inputs for human-only landing and cleanup commands. */
export type HumanCommandInput = {
  cwd: string
  sprint?: string
  lastSprint?: boolean
  ignoreNextBranch?: boolean
  dryRun: boolean
  json: boolean
}

/** Inputs needed to land a finalized sprint review branch onto a target. */
export type LandInput = HumanCommandInput & {
  target: string
}

/** Inputs needed to remove landed sprint branches and private sprint state. */
export type CleanupInput = HumanCommandInput & {
  target: string
  force: boolean
}

/** One sprint state file that can be landed or cleaned up. */
export type SprintCandidate = {
  sprint: string
  stateRelativePath: string
  reviewBranch: string
  lastActedAt: string | null
  state: SprintBranchState
}

/** A clean worktree that must release a sprint branch before that branch is deleted. */
export type SprintBranchWorktree = {
  path: string
  head: string | null
  branch: string | null
  detached: boolean
  reason: string
}

/** Parsed entry from git worktree list --porcelain. */
export type WorktreeEntry = {
  path: string
  head: string | null
  branch: string | null
  detached: boolean
}

/** Common report fields for human-only landing commands. */
export type HumanCommandReport = {
  ok: boolean
  command: "land" | "cleanup"
  dryRun: boolean
  executed: boolean
  sprint: string | null
  targetBranch: string
  currentBranch: string | null
  reviewBranch: string | null
  reviewCommit: string | null
  gitOperations: string[]
  diagnostics: SprintDiagnostic[]
  candidates: Array<{
    sprint: string
    statePath: string
    reviewBranch: string
  }>
}

/** Report returned after planning or running a finalized sprint landing. */
export type SprintLandReport = HumanCommandReport & {
  command: "land"
}

/** Report returned after planning or running landed sprint cleanup. */
export type SprintCleanupReport = HumanCommandReport & {
  command: "cleanup"
  branchesToDelete: string[]
  worktreesToDetach: SprintBranchWorktree[]
  stateFilesToRemove: string[]
  backupPathsToRemove: string[]
}

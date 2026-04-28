/** Valid sprint branch roles managed by the sprint-branch workflow. */
export type SprintBranchRole = "review" | "approved" | "next"

/** Branch names recorded for one sprint branch workflow instance. */
export type SprintBranchNames = Record<SprintBranchRole, string>

/** Task ownership recorded for the rolling sprint branches. */
export type SprintTaskState = {
  review: string | null
  next: string | null
  approved: string[]
  finishedUnreviewed: string[]
}

/** A stash created by the CLI while interrupting next-branch work. */
export type SprintActiveStash = {
  ref?: string
  sourceBranch?: string
  task?: string | null
  reason?: string
  message?: string
}

/** Lock metadata written while one mutating command owns sprint branch state. */
export type SprintLockState = {
  command: string
  createdAt: string
  pid: number
}

/** Conflict metadata recorded when a sprint branch transition stops early. */
export type SprintConflictState = {
  command?: string
  branch?: string
  message?: string
  [key: string]: unknown
}

/** Canonical machine-readable state stored in .sprint-branch-state.json. */
export type SprintBranchState = {
  schemaVersion: 1
  sprint: string
  baseBranch: string
  branches: SprintBranchNames
  tasks: SprintTaskState
  activeStashes: SprintActiveStash[]
  lock: SprintLockState | null
  conflict: SprintConflictState | null
}

/** Diagnostic emitted when the CLI detects an unsafe or inconsistent state. */
export type SprintDiagnostic = {
  severity: "error" | "warning" | "info"
  code: string
  message: string
  suggestion?: string
}

/** Resolved repository and sprint context used by commands. */
export type SprintContext = {
  rootDir: string
  sprint: string
  statePath: string
  stateRelativePath: string
  currentBranch: string | null
  inferredFrom: string
}

/** Git branch inspection result for one recorded sprint branch. */
export type SprintBranchStatus = {
  name: string
  exists: boolean
  head: string | null
}

/** Current working tree state from git status --porcelain. */
export type SprintWorkingTreeStatus = {
  clean: boolean
  entries: string[]
}

/** Human-readable index mirror inspection result. */
export type SprintIndexStatus = {
  path: string
  relativePath: string
  exists: boolean
  diverged: boolean
  warnings: string[]
}

/** Full status payload returned by sprint-branch status and reused by doctor. */
export type SprintStatusReport = {
  ok: boolean
  rootDir: string
  sprint: string
  statePath: string
  stateRelativePath: string
  currentBranch: string | null
  inferredFrom: string
  state: SprintBranchState
  branches: Record<SprintBranchRole, SprintBranchStatus>
  ancestry: {
    reviewDescendsFromApproved: boolean | null
    nextDescendsFromReview: boolean | null
  }
  workingTree: SprintWorkingTreeStatus
  index: SprintIndexStatus
  blocked: {
    review: boolean
    conflict: boolean
    feedback: boolean
    reasons: string[]
    nextSafeCommand?: string
  }
  diagnostics: SprintDiagnostic[]
}

/** Dry-run or execution report returned by mutating sprint commands. */
export type SprintMutationReport = {
  ok: boolean
  command: string
  dryRun: boolean
  executed: boolean
  sprint: string
  currentBranch: string | null
  summary: string
  requiresCleanWorkingTree: boolean
  gitOperations: string[]
  sprintFiles: string[]
  conflictHandling: string
  diagnostics: SprintDiagnostic[]
  state: SprintBranchState | null
}

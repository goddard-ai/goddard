import path from "node:path"
import { confirm, isCancel, select } from "@clack/prompts"

import {
  branchExists,
  getBranchHead,
  getCurrentBranch,
  getWorkingTreeStatus,
  GitCommandError,
  isAncestor,
  resolveRepositoryRoot,
  runGit,
} from "./git"
import {
  findSprintStateFiles,
  parseSprintBranchName,
  readSprintStateFile,
  sprintStatePath,
  validateSprintName,
} from "./state"
import type { SprintBranchState, SprintDiagnostic } from "./types"

/** Shared inputs for human-only landing and cleanup commands. */
type HumanCommandInput = {
  cwd: string
  sprint?: string
  dryRun: boolean
  json: boolean
}

/** Inputs needed to land a finalized sprint review branch onto a target. */
type LandInput = HumanCommandInput & {
  target: string
}

/** Inputs needed to remove landed sprint branches and review worktrees. */
type CleanupInput = HumanCommandInput & {
  target: string
}

/** One sprint state file that can be landed or cleaned up. */
type SprintCandidate = {
  sprint: string
  stateRelativePath: string
  reviewBranch: string
  state: SprintBranchState
}

/** A clean worktree that belongs to the landed sprint and can be removed. */
type AssociatedWorktree = {
  path: string
  head: string | null
  branch: string | null
  detached: boolean
  reason: string
}

/** Parsed entry from git worktree list --porcelain. */
type WorktreeEntry = {
  path: string
  head: string | null
  branch: string | null
  detached: boolean
}

/** Common report fields for human-only landing commands. */
type HumanCommandReport = {
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
  worktreesToRemove: AssociatedWorktree[]
}

/** Fast-forwards a human-selected target branch to the finalized review branch. */
export async function runLand(input: LandInput) {
  const rootDir = await resolveRepositoryRoot(input.cwd)
  const currentBranch = await getCurrentBranch(rootDir)
  const diagnostics: SprintDiagnostic[] = []
  const candidate = await resolveSprintCandidate(rootDir, input, currentBranch, diagnostics)
  const state = candidate?.state ?? null
  const reviewBranch = state?.branches.review ?? null
  const reviewCommit = reviewBranch ? await getBranchHead(rootDir, reviewBranch) : null
  const targetCommit = await getBranchHead(rootDir, input.target)
  const gitOperations = reviewBranch
    ? [`git checkout ${input.target}`, `git merge --ff-only ${reviewBranch}`]
    : []

  await pushLandingDiagnostics(rootDir, input, state, reviewCommit, targetCommit, diagnostics)

  const report = {
    ok: !diagnostics.some((diagnostic) => diagnostic.severity === "error"),
    command: "land" as const,
    dryRun: input.dryRun,
    executed: false,
    sprint: state?.sprint ?? null,
    targetBranch: input.target,
    currentBranch,
    reviewBranch,
    reviewCommit,
    gitOperations,
    diagnostics,
    candidates: candidate ? [] : await candidatesForOutput(rootDir),
  } satisfies SprintLandReport

  if (input.dryRun || !report.ok || !state) {
    return report
  }
  if (!(await confirmHumanAction(input, diagnostics, "Land finalized sprint review?"))) {
    return {
      ...report,
      ok: false,
      diagnostics,
    } satisfies SprintLandReport
  }

  try {
    await runGit(rootDir, ["checkout", input.target])
    await runGit(rootDir, ["merge", "--ff-only", state.branches.review])
    return { ...report, executed: true } satisfies SprintLandReport
  } catch (error) {
    return handleHumanGitError(report, error)
  }
}

/** Deletes landed sprint branches and clean associated worktrees after review is on target. */
export async function runCleanup(input: CleanupInput) {
  const rootDir = await resolveRepositoryRoot(input.cwd)
  const currentBranch = await getCurrentBranch(rootDir)
  const diagnostics: SprintDiagnostic[] = []
  const candidate = await resolveSprintCandidate(rootDir, input, currentBranch, diagnostics)
  const state = candidate?.state ?? null
  const reviewBranch = state?.branches.review ?? null
  const reviewCommit = reviewBranch ? await getBranchHead(rootDir, reviewBranch) : null
  const targetCommit = await getBranchHead(rootDir, input.target)
  const branchesToDelete = state ? await cleanupBranches(rootDir, state) : []
  const worktreesToRemove = state
    ? await associatedWorktrees(rootDir, state, reviewCommit, branchesToDelete, diagnostics)
    : []
  const gitOperations = [
    ...worktreesToRemove.map((worktree) => `git worktree remove ${JSON.stringify(worktree.path)}`),
    ...branchesToDelete.map((branch) => `git branch -d ${branch}`),
  ]

  await pushCleanupDiagnostics(
    rootDir,
    input,
    state,
    reviewCommit,
    targetCommit,
    branchesToDelete,
    diagnostics,
  )

  const report = {
    ok: !diagnostics.some((diagnostic) => diagnostic.severity === "error"),
    command: "cleanup" as const,
    dryRun: input.dryRun,
    executed: false,
    sprint: state?.sprint ?? null,
    targetBranch: input.target,
    currentBranch,
    reviewBranch,
    reviewCommit,
    gitOperations,
    diagnostics,
    candidates: candidate ? [] : await candidatesForOutput(rootDir),
    branchesToDelete,
    worktreesToRemove,
  } satisfies SprintCleanupReport

  if (input.dryRun || !report.ok || !state) {
    return report
  }
  if (!(await confirmHumanAction(input, diagnostics, "Delete landed sprint branches?"))) {
    return {
      ...report,
      ok: false,
      diagnostics,
    } satisfies SprintCleanupReport
  }

  try {
    for (const worktree of worktreesToRemove) {
      await runGit(rootDir, ["worktree", "remove", worktree.path])
    }
    for (const branch of branchesToDelete) {
      await runGit(rootDir, ["branch", "-d", branch])
    }
    return { ...report, executed: true } satisfies SprintCleanupReport
  } catch (error) {
    return handleHumanGitError(report, error)
  }
}

/** Formats human landing reports for terminal output. */
export function formatHumanCommandReport(report: SprintLandReport | SprintCleanupReport) {
  const lines = [
    `${report.dryRun ? "Dry run" : report.executed ? "Executed" : "Planned"}: ${report.command}`,
    `Sprint: ${report.sprint ?? "unknown"}`,
    `Target branch: ${report.targetBranch}`,
    `Current branch: ${report.currentBranch ?? "detached HEAD"}`,
    `Review branch: ${report.reviewBranch ?? "unknown"}`,
    `Review commit: ${report.reviewCommit?.slice(0, 12) ?? "unknown"}`,
    "",
    "Git operations:",
    ...formatList(report.gitOperations),
  ]

  if (report.command === "cleanup") {
    lines.push(
      "",
      "Branches to delete:",
      ...formatList(report.branchesToDelete),
      "",
      "Worktrees to remove:",
      ...formatList(report.worktreesToRemove.map((worktree) => worktree.path)),
    )
  }

  if (report.candidates.length > 0) {
    lines.push("", "Candidates:")
    for (const candidate of report.candidates) {
      lines.push(`  - ${candidate.sprint}: ${candidate.reviewBranch}`)
    }
  }

  if (report.diagnostics.length > 0) {
    lines.push("", "Diagnostics:")
    for (const diagnostic of report.diagnostics) {
      lines.push(`  [${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`)
      if (diagnostic.suggestion) {
        lines.push(`    suggestion: ${diagnostic.suggestion}`)
      }
    }
  }

  return lines.join("\n")
}

async function pushLandingDiagnostics(
  rootDir: string,
  input: LandInput,
  state: SprintBranchState | null,
  reviewCommit: string | null,
  targetCommit: string | null,
  diagnostics: SprintDiagnostic[],
) {
  await pushSharedFinalizedDiagnostics(
    rootDir,
    input,
    state,
    reviewCommit,
    targetCommit,
    diagnostics,
  )
  if (
    state &&
    reviewCommit &&
    targetCommit &&
    !(await isAncestor(rootDir, input.target, state.branches.review))
  ) {
    diagnostics.push({
      severity: "error",
      code: "target_not_ancestor_of_review",
      message: `${input.target} cannot fast-forward to ${state.branches.review}.`,
      suggestion: `Run sprint-branch finalize --override-base ${input.target} before landing.`,
    })
  }
}

async function pushCleanupDiagnostics(
  rootDir: string,
  input: CleanupInput,
  state: SprintBranchState | null,
  reviewCommit: string | null,
  targetCommit: string | null,
  branchesToDelete: string[],
  diagnostics: SprintDiagnostic[],
) {
  await pushSharedFinalizedDiagnostics(
    rootDir,
    input,
    state,
    reviewCommit,
    targetCommit,
    diagnostics,
  )
  if (
    state &&
    reviewCommit &&
    targetCommit &&
    !(await isAncestor(rootDir, state.branches.review, input.target))
  ) {
    diagnostics.push({
      severity: "error",
      code: "target_missing_review",
      message: `${input.target} does not contain finalized review commit ${reviewCommit.slice(0, 12)}.`,
      suggestion: `Run sprint-branch land ${input.target} ${state.sprint} before cleanup.`,
    })
  }
  if (currentBranchIsDeleted(await getCurrentBranch(rootDir), branchesToDelete)) {
    diagnostics.push({
      severity: "error",
      code: "current_branch_would_be_deleted",
      message: "The current branch is one of the sprint branches cleanup would delete.",
      suggestion: `Check out ${input.target} before cleanup.`,
    })
  }
}

async function pushSharedFinalizedDiagnostics(
  rootDir: string,
  input: HumanCommandInput & { target: string },
  state: SprintBranchState | null,
  reviewCommit: string | null,
  targetCommit: string | null,
  diagnostics: SprintDiagnostic[],
) {
  const workingTree = await getWorkingTreeStatus(rootDir)
  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: `${input.dryRun ? "dry-run" : "command"} requires a clean working tree.`,
    })
  }
  if (!input.dryRun && input.json) {
    diagnostics.push({
      severity: "error",
      code: "interactive_json_not_supported",
      message: "land and cleanup only support --json with --dry-run.",
    })
  }
  if (!input.dryRun && (!process.stdin.isTTY || !process.stdout.isTTY)) {
    diagnostics.push({
      severity: "error",
      code: "interactive_tty_required",
      message: "land and cleanup require an interactive terminal.",
    })
  }
  if (!(await branchExists(rootDir, input.target))) {
    diagnostics.push({
      severity: "error",
      code: "target_branch_missing",
      message: `Target branch ${input.target} does not exist.`,
    })
  }
  if (parseSprintBranchName(input.target)) {
    diagnostics.push({
      severity: "error",
      code: "target_is_sprint_branch",
      message: `${input.target} is a sprint branch and cannot be used as a landing target.`,
    })
  }
  if (!state) {
    return
  }

  const approvedCommit = await getBranchHead(rootDir, state.branches.approved)
  const nextCommit = await getBranchHead(rootDir, state.branches.next)
  if (state.conflict) {
    diagnostics.push({
      severity: "error",
      code: "conflict_recorded",
      message: `Sprint state records an unresolved ${state.conflict.command ?? "unknown"} conflict.`,
    })
  }
  if (state.tasks.review || state.tasks.next || state.tasks.finishedUnreviewed.length > 0) {
    diagnostics.push({
      severity: "error",
      code: "unreviewed_work_exists",
      message: "Landing requires no review task, no next task, and no finished unreviewed tasks.",
    })
  }
  if (state.activeStashes.length > 0) {
    diagnostics.push({
      severity: "error",
      code: "active_stashes_exist",
      message: "Landing requires all sprint interruption stashes to be resumed or resolved.",
      suggestion: "Run sprint-branch resume before landing.",
    })
  }
  if (!reviewCommit) {
    diagnostics.push({
      severity: "error",
      code: "review_branch_missing",
      message: `Review branch ${state.branches.review} does not exist.`,
    })
  }
  if (!approvedCommit) {
    diagnostics.push({
      severity: "error",
      code: "approved_branch_missing",
      message: `Approved branch ${state.branches.approved} does not exist.`,
    })
  }
  if (reviewCommit && approvedCommit && reviewCommit !== approvedCommit) {
    diagnostics.push({
      severity: "error",
      code: "review_approved_mismatch",
      message: `${state.branches.review} and ${state.branches.approved} do not point at the same finalized content.`,
    })
  }
  if (nextCommit && reviewCommit && nextCommit !== reviewCommit) {
    diagnostics.push({
      severity: "error",
      code: "active_next_branch_exists",
      message: `${state.branches.next} still points at content different from review.`,
    })
  }
  if (!targetCommit) {
    diagnostics.push({
      severity: "error",
      code: "target_branch_unresolved",
      message: `Target branch ${input.target} could not be resolved.`,
    })
  }
}

async function confirmHumanAction(
  input: HumanCommandInput,
  diagnostics: SprintDiagnostic[],
  message: string,
) {
  if (!process.stdin.isTTY || !process.stdout.isTTY || input.json) {
    return false
  }

  const confirmed = await confirm({ message, initialValue: false })
  if (isCancel(confirmed) || !confirmed) {
    diagnostics.push({
      severity: "error",
      code: "human_confirmation_required",
      message: "Command cancelled before mutating branches or worktrees.",
    })
    return false
  }

  return true
}

async function resolveSprintCandidate(
  rootDir: string,
  input: HumanCommandInput,
  currentBranch: string | null,
  diagnostics: SprintDiagnostic[],
) {
  if (input.sprint) {
    return readExplicitCandidate(rootDir, input.sprint, diagnostics)
  }

  const candidates = await readSprintCandidates(rootDir)
  const inferred = inferCandidate(rootDir, input.cwd, currentBranch, candidates)
  if (inferred) {
    return inferred
  }
  if (candidates.length === 0) {
    diagnostics.push({
      severity: "error",
      code: "missing_sprint_state",
      message: "No sprints/*/.sprint-branch-state.json files were found.",
    })
    return null
  }
  if (candidates.length === 1) {
    return candidates[0]
  }
  if (input.json || !process.stdin.isTTY || !process.stdout.isTTY) {
    diagnostics.push({
      severity: "error",
      code: "ambiguous_sprint",
      message: "Multiple sprints are available. Pass the sprint name as an argument.",
    })
    return null
  }

  const selected = await select({
    message: "Select sprint",
    options: candidates.map((candidate) => ({
      value: candidate.sprint,
      label: candidate.sprint,
      hint: candidate.reviewBranch,
    })),
  })

  if (isCancel(selected)) {
    diagnostics.push({
      severity: "error",
      code: "sprint_selection_cancelled",
      message: "Sprint selection cancelled.",
    })
    return null
  }

  return candidates.find((candidate) => candidate.sprint === selected) ?? null
}

async function readExplicitCandidate(
  rootDir: string,
  sprint: string,
  diagnostics: SprintDiagnostic[],
) {
  const validation = validateSprintName(sprint)
  diagnostics.push(...validation)
  if (validation.some((diagnostic) => diagnostic.severity === "error")) {
    return null
  }

  const statePath = sprintStatePath(rootDir, sprint)
  try {
    const parsed = await readSprintStateFile(statePath)
    diagnostics.push(...parsed.diagnostics)
    return parsed.state ? candidateFromState(rootDir, statePath, parsed.state) : null
  } catch (error) {
    if (isMissingFileError(error)) {
      diagnostics.push({
        severity: "error",
        code: "missing_sprint_state",
        message: `Sprint state sprints/${sprint}/.sprint-branch-state.json does not exist.`,
      })
      return null
    }
    throw error
  }
}

async function readSprintCandidates(rootDir: string) {
  const stateFiles = await findSprintStateFiles(rootDir)
  const candidates: SprintCandidate[] = []

  for (const statePath of stateFiles) {
    try {
      const parsed = await readSprintStateFile(statePath)
      if (parsed.state) {
        candidates.push(candidateFromState(rootDir, statePath, parsed.state))
      }
    } catch {
      continue
    }
  }

  return candidates.sort((left, right) => left.sprint.localeCompare(right.sprint))
}

async function candidatesForOutput(rootDir: string) {
  return (await readSprintCandidates(rootDir)).map((candidate) => ({
    sprint: candidate.sprint,
    statePath: candidate.stateRelativePath,
    reviewBranch: candidate.reviewBranch,
  }))
}

function inferCandidate(
  rootDir: string,
  cwd: string,
  currentBranch: string | null,
  candidates: SprintCandidate[],
) {
  if (currentBranch) {
    const branchSprint = parseSprintBranchName(currentBranch)?.sprint
    const candidate = candidates.find((entry) => entry.sprint === branchSprint)
    if (candidate) {
      return candidate
    }
  }

  const pathSprint = inferSprintFromPath(rootDir, cwd)
  return candidates.find((entry) => entry.sprint === pathSprint) ?? null
}

function candidateFromState(
  rootDir: string,
  statePath: string,
  state: SprintBranchState,
): SprintCandidate {
  return {
    sprint: state.sprint,
    stateRelativePath: path.relative(rootDir, statePath),
    reviewBranch: state.branches.review,
    state,
  }
}

function inferSprintFromPath(rootDir: string, cwd: string) {
  const relative = path.relative(path.join(rootDir, "sprints"), cwd)
  if (relative === "" || relative.startsWith("..") || path.isAbsolute(relative)) {
    return null
  }

  const [sprint] = relative.split(path.sep)
  return sprint || null
}

async function cleanupBranches(rootDir: string, state: SprintBranchState) {
  const branches = [state.branches.review, state.branches.approved]
  if (await branchExists(rootDir, state.branches.next)) {
    branches.push(state.branches.next)
  }
  return branches
}

async function associatedWorktrees(
  rootDir: string,
  state: SprintBranchState,
  reviewCommit: string | null,
  branchesToDelete: string[],
  diagnostics: SprintDiagnostic[],
) {
  const currentRoot = path.resolve(rootDir)
  const branchSet = new Set(branchesToDelete)
  const worktrees: AssociatedWorktree[] = []

  for (const worktree of await listWorktrees(rootDir)) {
    const resolvedPath = path.resolve(worktree.path)
    const branchMatch = worktree.branch && branchSet.has(worktree.branch)
    const detachedReview = Boolean(
      worktree.detached && reviewCommit && worktree.head === reviewCommit,
    )
    if (!branchMatch && !detachedReview) {
      continue
    }

    const reason = branchMatch ? `branch ${worktree.branch}` : "detached review snapshot"
    if (resolvedPath === currentRoot) {
      diagnostics.push({
        severity: "error",
        code: "current_worktree_would_be_removed",
        message: `Current worktree is associated with the sprint by ${reason}.`,
      })
      continue
    }

    const status = await getWorkingTreeStatus(worktree.path)
    if (!status.clean) {
      diagnostics.push({
        severity: "error",
        code: "dirty_associated_worktree",
        message: `Associated worktree ${worktree.path} is dirty.`,
      })
      continue
    }

    worktrees.push({
      ...worktree,
      reason,
    })
  }

  return worktrees
}

async function listWorktrees(rootDir: string) {
  const output = await runGit(rootDir, ["worktree", "list", "--porcelain"])
  const entries: WorktreeEntry[] = []
  let current: WorktreeEntry | null = null

  for (const line of output.split("\n")) {
    if (line.length === 0) {
      if (current) {
        entries.push(current)
        current = null
      }
      continue
    }
    if (line.startsWith("worktree ")) {
      if (current) {
        entries.push(current)
      }
      current = {
        path: line.slice("worktree ".length),
        head: null,
        branch: null,
        detached: false,
      }
      continue
    }
    if (!current) {
      continue
    }
    if (line.startsWith("HEAD ")) {
      current.head = line.slice("HEAD ".length)
    } else if (line.startsWith("branch refs/heads/")) {
      current.branch = line.slice("branch refs/heads/".length)
    } else if (line === "detached") {
      current.detached = true
    }
  }

  if (current) {
    entries.push(current)
  }

  return entries
}

function currentBranchIsDeleted(currentBranch: string | null, branchesToDelete: string[]) {
  return Boolean(currentBranch && branchesToDelete.includes(currentBranch))
}

function handleHumanGitError<T extends SprintLandReport | SprintCleanupReport>(
  report: T,
  error: unknown,
) {
  if (error instanceof GitCommandError) {
    return {
      ...report,
      ok: false,
      executed: true,
      diagnostics: [
        ...report.diagnostics,
        {
          severity: "error" as const,
          code: "git_operation_failed",
          message: error.stderr || error.message,
        },
      ],
    } satisfies T
  }
  throw error
}

function formatList(values: string[]) {
  if (values.length === 0) {
    return ["  none"]
  }
  return values.map((value) => `  - ${value}`)
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}

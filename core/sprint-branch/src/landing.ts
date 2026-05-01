import * as fs from "node:fs/promises"

import { runGit } from "./git/command"
import { getBranchHead } from "./git/refs"
import { getCurrentBranch, resolveRepositoryRoot } from "./git/repository"
import { confirmHumanAction } from "./landing/confirmation"
import { formatHumanCommandReport, handleHumanGitError } from "./landing/report"
import { candidatesForOutput, resolveSprintCandidate } from "./landing/selection"
import type {
  AssociatedWorktree,
  CleanupInput,
  LandInput,
  SprintCleanupReport,
  SprintLandReport,
} from "./landing/types"
import { pushCleanupDiagnostics, pushLandingDiagnostics } from "./landing/validation"
import { associatedWorktrees, cleanupBranches } from "./landing/worktrees"
import { sprintStateDisplayPath, sprintStatePath } from "./state/paths"
import type { SprintBranchState, SprintDiagnostic } from "./types"

export { formatHumanCommandReport } from "./landing/report"
export type { SprintCleanupReport, SprintLandReport } from "./landing/types"

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
  const stateFileToRemove = state ? sprintStateDisplayPath(state.sprint) : null
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
    stateFilesToRemove: stateFileToRemove ? [stateFileToRemove] : [],
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
    await executeCleanupOperations(rootDir, state, branchesToDelete, worktreesToRemove)
    return { ...report, executed: true } satisfies SprintCleanupReport
  } catch (error) {
    return handleHumanGitError(report, error)
  }
}

/** Executes already-confirmed cleanup of sprint refs, worktrees, and Git-private state. */
export async function executeCleanupOperations(
  rootDir: string,
  state: Pick<SprintBranchState, "sprint">,
  branchesToDelete: string[],
  worktreesToRemove: AssociatedWorktree[],
) {
  for (const worktree of worktreesToRemove) {
    await runGit(rootDir, ["worktree", "remove", worktree.path])
  }
  for (const branch of branchesToDelete) {
    await runGit(rootDir, ["branch", "-d", branch])
  }
  await fs.rm(await sprintStatePath(rootDir, state.sprint), { force: true })
}

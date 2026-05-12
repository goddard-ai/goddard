import * as fs from "node:fs/promises"

import { hasDiagnosticErrors } from "./diagnostics"
import { GitCommandError, runGit } from "./git/command"
import { getBranchHead } from "./git/refs"
import { getCurrentBranch, resolveRepositoryRoot } from "./git/repository"
import { confirmHumanAction } from "./landing/confirmation"
import { handleHumanGitError } from "./landing/report"
import { candidatesForOutput, resolveSprintCandidate } from "./landing/selection"
import type {
  CleanupInput,
  LandInput,
  SprintBranchWorktree,
  SprintCleanupReport,
  SprintLandReport,
} from "./landing/types"
import {
  pushCleanupDiagnostics,
  pushLandingDiagnostics,
  pushTargetBranchDiagnostics,
} from "./landing/validation"
import { cleanupBranches, listWorktrees, sprintBranchWorktrees } from "./landing/worktrees"
import { writeSprintLastActedAt } from "./state/activity"
import { sprintStateDisplayPath, sprintStatePath } from "./state/paths"
import type { SprintBranchState, SprintDiagnostic } from "./types"

export { formatHumanCommandReport } from "./landing/report"
export type { SprintCleanupReport, SprintLandReport } from "./landing/types"

/** Fast-forwards a human-selected target branch to the finalized review branch. */
export async function runLand(input: LandInput) {
  const rootDir = await resolveRepositoryRoot(input.cwd)
  const currentBranch = await getCurrentBranch(rootDir)
  const diagnostics: SprintDiagnostic[] = []
  const targetCommit = await getBranchHead(rootDir, input.target)
  pushTargetBranchDiagnostics(input, targetCommit, diagnostics)
  if (hasDiagnosticErrors(diagnostics)) {
    return {
      ok: false,
      command: "land" as const,
      dryRun: input.dryRun,
      executed: false,
      sprint: null,
      targetBranch: input.target,
      currentBranch,
      reviewBranch: null,
      reviewCommit: null,
      gitOperations: [],
      diagnostics,
      candidates: await candidatesForOutput(rootDir, {
        finalizedOutputOnly: true,
        ignoreNextBranch: input.ignoreNextBranch,
      }),
    } satisfies SprintLandReport
  }

  const candidate = await resolveSprintCandidate(rootDir, input, currentBranch, diagnostics, {
    finalizedPromptOnly: true,
    ignoreNextBranch: input.ignoreNextBranch,
  })
  const state = candidate?.state ?? null
  const reviewBranch = state?.branches.review ?? null
  const reviewCommit = reviewBranch ? await getBranchHead(rootDir, reviewBranch) : null
  const gitOperations = reviewBranch
    ? [`git checkout ${input.target}`, `git merge --ff-only ${reviewBranch}`]
    : []

  await pushLandingDiagnostics(rootDir, input, state, reviewCommit, targetCommit, diagnostics)

  const report = {
    ok: !hasDiagnosticErrors(diagnostics),
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
    candidates: candidate
      ? []
      : await candidatesForOutput(rootDir, {
          finalizedOutputOnly: true,
          ignoreNextBranch: input.ignoreNextBranch,
        }),
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
    await executeLandOperations(rootDir, input.target, state.branches.review)
    await writeSprintLastActedAt(rootDir, state)
    return { ...report, executed: true } satisfies SprintLandReport
  } catch (error) {
    return handleHumanGitError(report, error)
  }
}

/** Executes an already-confirmed landing merge in the worktree that can own the target. */
export async function executeLandOperations(
  rootDir: string,
  targetBranch: string,
  reviewBranch: string,
) {
  try {
    await runGit(rootDir, ["checkout", targetBranch])
    await runGit(rootDir, ["merge", "--ff-only", reviewBranch])
    return
  } catch (error) {
    if (!isTargetBranchUsedByAnotherWorktree(error, targetBranch)) {
      throw error
    }
  }

  const targetWorktree = (await listWorktrees(rootDir)).find(
    (worktree) => worktree.branch === targetBranch,
  )
  if (!targetWorktree) {
    throw new GitCommandError(["checkout", targetBranch], {
      stderr: `Target branch ${targetBranch} is already checked out, but its worktree could not be found.`,
      code: 1,
    })
  }
  await runGit(targetWorktree.path, ["merge", "--ff-only", reviewBranch])
}

/** Deletes landed sprint branches and private sprint state after review is on target. */
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
  const worktreesToDetach = state
    ? await sprintBranchWorktrees(rootDir, branchesToDelete, diagnostics)
    : []
  const gitOperations = [
    ...worktreesToDetach.map(
      (worktree) => `git -C ${JSON.stringify(worktree.path)} checkout --detach`,
    ),
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
    ok: !hasDiagnosticErrors(diagnostics),
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
    worktreesToDetach,
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
    await executeCleanupOperations(rootDir, state, branchesToDelete, worktreesToDetach)
    return { ...report, executed: true } satisfies SprintCleanupReport
  } catch (error) {
    return handleHumanGitError(report, error)
  }
}

/** Executes already-confirmed cleanup of sprint refs and Git-private state. */
export async function executeCleanupOperations(
  rootDir: string,
  state: Pick<SprintBranchState, "sprint">,
  branchesToDelete: string[],
  worktreesToDetach: SprintBranchWorktree[],
) {
  for (const worktree of worktreesToDetach) {
    await runGit(worktree.path, ["checkout", "--detach"])
  }
  for (const branch of branchesToDelete) {
    await runGit(rootDir, ["branch", "-d", branch])
  }
  await fs.rm(await sprintStatePath(rootDir, state.sprint), { force: true })
}

function isTargetBranchUsedByAnotherWorktree(error: unknown, targetBranch: string) {
  return (
    error instanceof GitCommandError &&
    error.args[0] === "checkout" &&
    error.args[1] === targetBranch &&
    error.stderr.includes(`'${targetBranch}' is already used by worktree`)
  )
}

import * as fs from "node:fs/promises"
import path from "node:path"

import { GitCommandError, runGit } from "./git/command"
import { branchExists, getBranchHead, isAncestor } from "./git/refs"
import { getStashRefs } from "./git/stash"
import { getWorkingTreeStatus } from "./git/worktree"
import { getExpectedBranches } from "./state/branches"
import { inferSprintContext } from "./state/inference"
import { readTransientConflict } from "./transient-conflict"
import type { SprintActiveStash, SprintBranchState, SprintDiagnostic } from "./types"
import { moveBranchOperation, moveRecordedBranch } from "./workflow/branch-movement"
import { readCommandState } from "./workflow/command-state"
import {
  conflictReport,
  hasUnmergedEntries,
  isRetryingCommand,
  pushActiveGitOperationDiagnostics,
  writeConflictStateWhenSafe,
} from "./workflow/conflicts"
import { withSprintLock } from "./workflow/lock"
import {
  formatMutationReport,
  makePlan,
  SprintMutationError,
  withDryRun,
  type MutationInput,
} from "./workflow/report"
import { stateFilesForState, writeSprintState } from "./workflow/state-files"
import {
  cloneState,
  emptyTasks,
  findMatchingStash,
  nextTaskDiagnostic,
  noPlannedTaskDiagnostic,
  normalizeTaskName,
  resolveNextPlannedTask,
  taskFileExists,
} from "./workflow/tasks"

export { formatMutationReport, SprintMutationError }

/** Initializes canonical sprint branch state and the review/approved branch scaffold. */
export async function runInit(input: MutationInput & { base: string }) {
  const context = await inferSprintContext(input)
  const branches = getExpectedBranches(context.sprint)
  const state: SprintBranchState = {
    schemaVersion: 1,
    sprint: context.sprint,
    baseBranch: input.base,
    branches,
    tasks: emptyTasks(),
    activeStashes: [],
    lock: null,
    conflict: null,
  }
  const diagnostics: SprintDiagnostic[] = []
  const sprintDir = path.join(context.rootDir, "sprints", context.sprint)

  if (!(await pathExists(sprintDir))) {
    diagnostics.push({
      severity: "error",
      code: "sprint_folder_missing",
      message: `Sprint folder sprints/${context.sprint} does not exist.`,
    })
  }
  if (await pathExists(context.statePath)) {
    diagnostics.push({
      severity: "error",
      code: "state_already_exists",
      message: `${context.stateRelativePath} already exists.`,
    })
  }
  if (!(await branchExists(context.rootDir, input.base))) {
    diagnostics.push({
      severity: "error",
      code: "base_branch_missing",
      message: `Base branch ${input.base} does not exist.`,
    })
  }
  await pushIfBranchExists(
    context.rootDir,
    `sprint/${context.sprint}`,
    diagnostics,
    "bare_sprint_branch_exists",
    `Bare sprint namespace branch sprint/${context.sprint} must not exist.`,
  )
  for (const branch of [branches.approved, branches.review, branches.next]) {
    await pushIfBranchExists(
      context.rootDir,
      branch,
      diagnostics,
      "sprint_branch_exists",
      `Sprint branch ${branch} already exists.`,
    )
  }

  const plan = makePlan({
    command: "init",
    context,
    state,
    summary: `Initialize sprint ${context.sprint} from ${input.base}.`,
    requiresCleanWorkingTree: false,
    gitOperations: [
      `git branch ${branches.approved} ${input.base}`,
      `git branch ${branches.review} ${branches.approved}`,
    ],
    stateFiles: stateFilesForState(state),
    conflictHandling: "No rebase is performed. Existing branches or state files stop the command.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "init", async () => {
    await runGit(context.rootDir, ["branch", branches.approved, input.base])
    await runGit(context.rootDir, ["branch", branches.review, branches.approved])
    await writeSprintState(context.rootDir, state)
    return { ...plan, executed: true }
  })
}

/** Starts the requested task on review or next according to the rolling branch state. */
export async function runStart(input: MutationInput & { task: string }) {
  const { context, state, diagnostics } = await readCommandState(input, "start")
  const task = normalizeTaskName(input.task)
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const plannedTask = await resolveNextPlannedTask(context.rootDir, state)
  const gitOperations: string[] = []
  let summary = ""
  let targetBranch = state.branches.review
  const nextState = cloneState(state)

  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: "start requires a clean working tree before moving sprint branches.",
    })
  }
  if (!(await taskFileExists(context.rootDir, state.sprint, task))) {
    diagnostics.push({
      severity: "error",
      code: "task_file_missing",
      message: `Task file sprints/${state.sprint}/${task}.md does not exist.`,
    })
  }

  if (!state.tasks.review) {
    if (!plannedTask) {
      diagnostics.push(noPlannedTaskDiagnostic(task))
    } else if (task !== plannedTask) {
      diagnostics.push(nextTaskDiagnostic(task, plannedTask))
    }
    summary = `Start ${task} on review.`
    gitOperations.push(
      moveBranchOperation(state.branches.review, state.branches.approved, context.currentBranch),
      `git checkout ${state.branches.review}`,
    )
    nextState.tasks.review = task
    targetBranch = state.branches.review
  } else if (state.tasks.review === task) {
    summary = `Continue ${task} on review.`
    gitOperations.push(`git checkout ${state.branches.review}`)
    targetBranch = state.branches.review
  } else if (!state.tasks.next) {
    if (!plannedTask) {
      diagnostics.push(noPlannedTaskDiagnostic(task))
    } else if (task !== plannedTask) {
      diagnostics.push(nextTaskDiagnostic(task, plannedTask))
    }
    summary = `Start ${task} as work-ahead on next.`
    gitOperations.push(
      moveBranchOperation(state.branches.next, state.branches.review, context.currentBranch),
      `git checkout ${state.branches.next}`,
    )
    nextState.tasks.next = task
    targetBranch = state.branches.next
  } else if (state.tasks.next === task) {
    summary = `Continue ${task} on next.`
    gitOperations.push(`git checkout ${state.branches.next}`)
    targetBranch = state.branches.next
  } else {
    diagnostics.push({
      severity: "error",
      code: "review_limit_reached",
      message: `Review is occupied by ${state.tasks.review} and next is occupied by ${state.tasks.next}.`,
      suggestion: "Run sprint-branch feedback, resume, or approve before starting another task.",
    })
    summary = `Cannot start ${task}; review and next are occupied.`
  }

  const plan = makePlan({
    command: "start",
    context,
    state: nextState,
    summary,
    requiresCleanWorkingTree: true,
    gitOperations,
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "No rebase is performed. Branch movement stops before files are updated on failure.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "start", async () => {
    if (targetBranch === state.branches.review && !state.tasks.review) {
      await moveRecordedBranch(
        context.rootDir,
        state,
        state.branches.review,
        state.branches.approved,
      )
    } else if (targetBranch === state.branches.next && state.tasks.review && !state.tasks.next) {
      await moveRecordedBranch(context.rootDir, state, state.branches.next, state.branches.review)
    }
    await runGit(context.rootDir, ["checkout", targetBranch])
    await writeSprintState(context.rootDir, nextState)
    return { ...plan, executed: true }
  })
}

/** Stashes interrupted next-branch work when needed and checks out the review branch. */
export async function runFeedback(input: MutationInput) {
  const { context, state, diagnostics } = await readCommandState(input, "feedback")
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const nextState = cloneState(state)
  const gitOperations: string[] = []
  const onNext = context.currentBranch === state.branches.next
  const message = `sprint-branch:${state.sprint}:${state.tasks.next ?? "no-task"}:feedback`

  if (!onNext && !workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_non_next_worktree",
      message: "feedback can only stash dirty work from the recorded next branch.",
    })
  }
  if (onNext && !state.tasks.next) {
    diagnostics.push({
      severity: "error",
      code: "next_task_missing",
      message: `${state.branches.next} is checked out but no next task is recorded.`,
    })
  }

  if (onNext && !workingTree.clean) {
    gitOperations.push(`git stash push --include-untracked -m ${JSON.stringify(message)}`)
  }
  gitOperations.push(`git checkout ${state.branches.review}`)

  const plan = makePlan({
    command: "feedback",
    context,
    state: nextState,
    summary: "Prepare the review branch for human feedback.",
    requiresCleanWorkingTree: false,
    gitOperations,
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "Stash failures leave the working tree unchanged. Checkout failures stop before state updates.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "feedback", async () => {
    let recordedStash: SprintActiveStash | null = null
    if (onNext && !workingTree.clean) {
      await runGit(context.rootDir, ["stash", "push", "--include-untracked", "-m", message])
      const latest = await getLatestStash(context.rootDir)
      recordedStash = {
        ref: latest?.ref,
        sourceBranch: state.branches.next,
        task: state.tasks.next,
        reason: "feedback",
        message,
      }
      nextState.activeStashes = [...nextState.activeStashes, recordedStash]
    }

    await runGit(context.rootDir, ["checkout", state.branches.review])
    await writeSprintState(context.rootDir, nextState)
    return { ...plan, state: nextState, executed: true }
  })
}

/** Rebases dependent next work after feedback and reapplies a matching recorded stash. */
export async function runResume(input: MutationInput) {
  const { context, state, diagnostics } = await readCommandState(input, "resume", {
    allowOwnConflictRetry: true,
  })
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const nextState = cloneState(state)
  const gitOperations: string[] = []
  const matchingStash = findMatchingStash(state)
  const retryingResume = isRetryingCommand(state, "resume")
  const resolvingStashApplyConflict =
    retryingResume &&
    Boolean(state.tasks.next) &&
    Boolean(matchingStash) &&
    context.currentBranch === state.branches.next &&
    !workingTree.clean &&
    !hasUnmergedEntries(workingTree.entries)
  let targetBranch = state.tasks.next ? state.branches.next : state.branches.review

  if (!workingTree.clean && !resolvingStashApplyConflict) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: "resume requires a clean working tree before rebasing or applying a recorded stash.",
    })
  }
  if (state.tasks.next && !(await branchExists(context.rootDir, state.branches.next))) {
    diagnostics.push({
      severity: "error",
      code: "next_branch_missing",
      message: `Next branch ${state.branches.next} does not exist.`,
    })
  }
  if (retryingResume && !resolvingStashApplyConflict) {
    await pushActiveGitOperationDiagnostics(context.rootDir, diagnostics)
  }

  if (resolvingStashApplyConflict) {
    gitOperations.push("clear recorded feedback stash after resolved stash-apply conflict")
  } else if (state.tasks.next) {
    gitOperations.push(`git checkout ${state.branches.next}`)
    if (!(await isAncestor(context.rootDir, state.branches.review, state.branches.next))) {
      gitOperations.push(`git rebase ${state.branches.review}`)
    }
    if (matchingStash?.ref) {
      gitOperations.push(`git stash apply ${matchingStash.ref}`)
    }
  } else {
    gitOperations.push(`git checkout ${state.branches.review}`)
    targetBranch = state.branches.review
  }

  const plan = makePlan({
    command: "resume",
    context,
    state: nextState,
    summary: state.tasks.next
      ? resolvingStashApplyConflict
        ? `Finish resume for ${state.tasks.next} after resolved stash conflict.`
        : `Resume ${state.tasks.next} on next.`
      : "No next task is recorded; return to review.",
    requiresCleanWorkingTree: !resolvingStashApplyConflict,
    gitOperations,
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "State remains pre-resume until rebase and stash application finish. Retry resume after resolving any recorded conflict.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "resume", async () => {
    try {
      if (resolvingStashApplyConflict) {
        nextState.activeStashes = matchingStash
          ? nextState.activeStashes.filter(
              (stash) =>
                stash.ref !== matchingStash.ref ||
                stash.sourceBranch !== matchingStash.sourceBranch ||
                stash.task !== matchingStash.task,
            )
          : nextState.activeStashes
        nextState.conflict = null
        await writeSprintState(context.rootDir, nextState)
        return { ...plan, state: nextState, executed: true }
      }

      await runGit(context.rootDir, ["checkout", targetBranch])
      if (
        state.tasks.next &&
        !(await isAncestor(context.rootDir, state.branches.review, state.branches.next))
      ) {
        await runGit(context.rootDir, ["rebase", state.branches.review])
      }
      if (state.tasks.next && matchingStash?.ref) {
        await runGit(context.rootDir, ["stash", "apply", matchingStash.ref])
        nextState.activeStashes = nextState.activeStashes.filter(
          (stash) =>
            stash.ref !== matchingStash.ref ||
            stash.sourceBranch !== matchingStash.sourceBranch ||
            stash.task !== matchingStash.task,
        )
      }
      nextState.conflict = null
      await writeSprintState(context.rootDir, nextState)
      return { ...plan, state: nextState, executed: true }
    } catch (error) {
      if (error instanceof GitCommandError) {
        const conflictState = await writeConflictStateWhenSafe(
          context.rootDir,
          state,
          "resume",
          targetBranch,
          error,
        )
        return conflictReport(plan, conflictState, error)
      }
      throw error
    }
  })
}

/** Promotes the review branch into approved and rolls existing next work onto review. */
export async function runApprove(input: MutationInput) {
  const { context, state, diagnostics } = await readCommandState(input, "approve", {
    allowOwnConflictRetry: true,
  })
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const nextState = cloneState(state)
  const gitOperations: string[] = []
  const retryingApprove = isRetryingCommand(state, "approve")
  const nextNeedsRebase =
    Boolean(state.tasks.next) &&
    !(await isAncestor(context.rootDir, state.branches.review, state.branches.next))

  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: "approve requires a clean working tree.",
    })
  }
  if (!state.tasks.review) {
    diagnostics.push({
      severity: "error",
      code: "review_task_missing",
      message: "No review task is recorded for approval.",
    })
  }
  if (
    state.tasks.review &&
    !(await isAncestor(context.rootDir, state.branches.approved, state.branches.review))
  ) {
    diagnostics.push({
      severity: "error",
      code: "review_not_based_on_approved",
      message: `${state.branches.review} does not descend from ${state.branches.approved}.`,
    })
  }
  if (retryingApprove) {
    await pushActiveGitOperationDiagnostics(context.rootDir, diagnostics)
  }

  if (state.tasks.next) {
    gitOperations.push(`git checkout ${state.branches.next}`)
    if (nextNeedsRebase) {
      gitOperations.push(`git rebase ${state.branches.review}`)
    }
    nextState.tasks.approved = [...nextState.tasks.approved, state.tasks.review].filter(
      (task): task is string => Boolean(task),
    )
    nextState.tasks.finishedUnreviewed = nextState.tasks.finishedUnreviewed.filter(
      (task) => task !== state.tasks.review,
    )
    nextState.tasks.review = state.tasks.next
    nextState.tasks.next = null
  }

  gitOperations.push(
    `git checkout ${state.branches.approved}`,
    `git merge --ff-only ${state.branches.review}`,
  )

  if (state.tasks.next) {
    gitOperations.push(
      moveBranchOperation(state.branches.review, state.branches.next, state.branches.approved),
      `git checkout ${state.branches.review}`,
    )
  } else {
    gitOperations.push(
      moveBranchOperation(state.branches.review, state.branches.approved, state.branches.approved),
      `git checkout ${state.branches.review}`,
    )
    nextState.tasks.approved = [...nextState.tasks.approved, state.tasks.review].filter(
      (task): task is string => Boolean(task),
    )
    nextState.tasks.finishedUnreviewed = nextState.tasks.finishedUnreviewed.filter(
      (task) => task !== state.tasks.review,
    )
    nextState.tasks.review = null
  }

  const plan = makePlan({
    command: "approve",
    context,
    state: nextState,
    summary: state.tasks.review
      ? `Approve ${state.tasks.review}.`
      : "Approve the current review task.",
    requiresCleanWorkingTree: true,
    gitOperations,
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "Validation and next rebasing happen before approved is moved. Retry approve after resolving any recorded Git conflict.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "approve", async () => {
    let conflictBranch = state.branches.approved
    try {
      if (state.tasks.next) {
        conflictBranch = state.branches.next
        await runGit(context.rootDir, ["checkout", state.branches.next])
        if (!(await isAncestor(context.rootDir, state.branches.review, state.branches.next))) {
          await runGit(context.rootDir, ["rebase", state.branches.review])
        }
      }

      conflictBranch = state.branches.approved
      await runGit(context.rootDir, ["checkout", state.branches.approved])
      await runGit(context.rootDir, ["merge", "--ff-only", state.branches.review])

      if (state.tasks.next) {
        await moveRecordedBranch(context.rootDir, state, state.branches.review, state.branches.next)
        await runGit(context.rootDir, ["checkout", state.branches.review])
      } else {
        await moveRecordedBranch(
          context.rootDir,
          state,
          state.branches.review,
          state.branches.approved,
        )
        await runGit(context.rootDir, ["checkout", state.branches.review])
      }
      nextState.conflict = null
      await writeSprintState(context.rootDir, nextState)
      return { ...plan, state: nextState, executed: true }
    } catch (error) {
      if (error instanceof GitCommandError) {
        const conflictState = await writeConflictStateWhenSafe(
          context.rootDir,
          state,
          "approve",
          conflictBranch,
          error,
        )
        return conflictReport(plan, conflictState, error)
      }
      throw error
    }
  })
}

/** Rebases the completed review branch onto base for the final human merge. */
export async function runFinalize(input: MutationInput & { overrideBase?: string }) {
  const { context, state, diagnostics } = await readCommandState(input, "finalize", {
    allowOwnConflictRetry: true,
  })
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const transientConflict = await readTransientConflict(context.rootDir, state.sprint)
  const nextState = cloneState(state)
  const retryingFinalize =
    isRetryingCommand(state, "finalize") || transientConflict?.command === "finalize"
  const conflictBaseBranch =
    retryingFinalize && typeof state.conflict?.baseBranch === "string"
      ? state.conflict.baseBranch
      : retryingFinalize && typeof transientConflict?.baseBranch === "string"
        ? transientConflict.baseBranch
        : undefined
  const baseBranch = input.overrideBase ?? conflictBaseBranch ?? state.baseBranch
  const reviewHead = await getBranchHead(context.rootDir, state.branches.review)
  const approvedHead = await getBranchHead(context.rootDir, state.branches.approved)
  const nextHead = await getBranchHead(context.rootDir, state.branches.next)

  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: "finalize requires a clean working tree.",
    })
  }
  if (state.tasks.review || state.tasks.next || state.tasks.finishedUnreviewed.length > 0) {
    diagnostics.push({
      severity: "error",
      code: "unreviewed_work_exists",
      message: "finalize requires no review task, no next task, and no finished unreviewed tasks.",
    })
  }
  if (reviewHead && approvedHead && reviewHead !== approvedHead && !retryingFinalize) {
    diagnostics.push({
      severity: "error",
      code: "review_approved_mismatch",
      message: `${state.branches.review} and ${state.branches.approved} do not point at the same approved content.`,
    })
  }
  if (nextHead && reviewHead && nextHead !== reviewHead) {
    diagnostics.push({
      severity: "error",
      code: "active_next_branch_exists",
      message: `${state.branches.next} still points at content different from review.`,
    })
  }
  if (!(await branchExists(context.rootDir, baseBranch))) {
    diagnostics.push({
      severity: "error",
      code: "base_branch_missing",
      message: `Base branch ${baseBranch} does not exist.`,
    })
  }
  if (retryingFinalize) {
    await pushActiveGitOperationDiagnostics(context.rootDir, diagnostics)
  }

  nextState.baseBranch = baseBranch
  const plan = makePlan({
    command: "finalize",
    context,
    state: nextState,
    summary: `Finalize ${state.branches.review} on ${baseBranch}.`,
    requiresCleanWorkingTree: true,
    gitOperations: [
      `git checkout ${state.branches.review}`,
      `git rebase ${baseBranch}`,
      moveBranchOperation(state.branches.approved, state.branches.review, state.branches.review),
    ],
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "State remains pre-finalize until the final rebase and approved ref update both succeed. Retry finalize after resolving any recorded conflict.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "finalize", async () => {
    try {
      await runGit(context.rootDir, ["checkout", state.branches.review])
      await runGit(context.rootDir, ["rebase", baseBranch])
      await moveRecordedBranch(
        context.rootDir,
        state,
        state.branches.approved,
        state.branches.review,
      )
      nextState.conflict = null
      await writeSprintState(context.rootDir, nextState)
      return { ...plan, state: nextState, executed: true }
    } catch (error) {
      if (error instanceof GitCommandError) {
        const conflictState = await writeConflictStateWhenSafe(
          context.rootDir,
          state,
          "finalize",
          state.branches.review,
          error,
          { baseBranch },
        )
        return conflictReport(plan, conflictState, error)
      }
      throw error
    }
  })
}

async function getLatestStash(rootDir: string) {
  const stashes = await getStashRefs(rootDir)
  const [first] = stashes.entries()
  if (!first) {
    return null
  }
  return {
    ref: first[0],
    message: first[1],
  }
}

async function pushIfBranchExists(
  rootDir: string,
  branch: string,
  diagnostics: SprintDiagnostic[],
  code: string,
  message: string,
) {
  if (await branchExists(rootDir, branch)) {
    diagnostics.push({
      severity: "error",
      code,
      message,
    })
  }
}

async function pathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}

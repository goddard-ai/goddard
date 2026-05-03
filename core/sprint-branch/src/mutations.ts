import * as fs from "node:fs/promises"
import path from "node:path"

import { GitCommandError, runGit } from "./git/command"
import { branchExists, getBranchHead, getMergeBase, isAncestor, refExists } from "./git/refs"
import { getGitOperations } from "./git/repository"
import { getStashRefs } from "./git/stash"
import { getWorkingTreeStatus } from "./git/worktree"
import { readTaskReviewReport } from "./review-report"
import { getExpectedBranches } from "./state/branches"
import { inferSprintContext } from "./state/inference"
import { readSprintStateFile } from "./state/io"
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
  listTaskStems,
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
    sprint: context.sprint,
    baseBranch: input.base,
    visibility: "active",
    branches,
    tasks: emptyTasks(),
    activeStashes: [],
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
  if (!(await refExists(context.rootDir, input.base))) {
    diagnostics.push({
      severity: "error",
      code: "base_branch_missing",
      message: `Base ref ${input.base} does not resolve to a commit.`,
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

/** Recreates sprint branch state so the selected task becomes the next start target. */
export async function runResetState(
  input: MutationInput & { task?: string; base?: string; force?: boolean },
) {
  const context = await inferSprintContext(input)
  const diagnostics: SprintDiagnostic[] = []
  const existingState = await readResetSeedState(context.statePath, diagnostics)
  const branches = getExpectedBranches(context.sprint)
  const baseBranch = input.base ?? existingState?.baseBranch ?? "main"
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const sprintDir = path.join(context.rootDir, "sprints", context.sprint)
  const taskStems = (await pathExists(sprintDir))
    ? await listTaskStems(context.rootDir, context.sprint)
    : []
  const targetTask = input.task ? normalizeTaskName(input.task) : (taskStems[0] ?? null)
  const targetTaskIndex = targetTask ? taskStems.indexOf(targetTask) : -1
  const nextState: SprintBranchState = {
    sprint: context.sprint,
    baseBranch,
    visibility: existingState?.visibility ?? "active",
    branches,
    tasks: {
      review: null,
      next: null,
      approved: targetTaskIndex >= 0 ? taskStems.slice(0, targetTaskIndex) : [],
      finishedUnreviewed: [],
    },
    activeStashes: [],
    conflict: null,
  }

  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: "reset-state requires a clean working tree before rewriting sprint state.",
    })
  }
  if (!(await pathExists(sprintDir))) {
    diagnostics.push({
      severity: "error",
      code: "sprint_folder_missing",
      message: `Sprint folder sprints/${context.sprint} does not exist.`,
    })
  }
  if (taskStems.length === 0) {
    diagnostics.push({
      severity: "error",
      code: "no_sprint_tasks",
      message: `Sprint folder sprints/${context.sprint} has no task files.`,
    })
  }
  if (targetTask && targetTaskIndex === -1) {
    diagnostics.push({
      severity: "error",
      code: "task_file_missing",
      message: `Task file sprints/${context.sprint}/${targetTask}.md does not exist.`,
    })
  }
  diagnostics.push(...duplicateTaskPrefixDiagnostics(taskStems))

  await pushResetBranchDiagnostics(
    context.rootDir,
    branches,
    baseBranch,
    Boolean(input.force),
    diagnostics,
  )
  pushResetStateDiagnostics(existingState, Boolean(input.force), diagnostics)

  const plan = makePlan({
    command: "reset-state",
    context,
    state: nextState,
    summary: targetTask
      ? `Reset sprint ${context.sprint} state so ${targetTask} is next.`
      : `Reset sprint ${context.sprint} state to the first task.`,
    requiresCleanWorkingTree: true,
    gitOperations: [],
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "Only Git-private sprint state is rewritten. Branches are not moved; use --force only after preserving or discarding branch-local work intentionally.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, nextState, "reset-state", async () => {
    await writeSprintState(context.rootDir, nextState)
    return { ...plan, executed: true }
  })
}

/** Parks a sprint so default selection ignores it until explicitly requested. */
export async function runPark(input: MutationInput) {
  return runVisibilityChange(input, "park", "parked")
}

/** Restores a parked sprint to default active selection. */
export async function runUnpark(input: MutationInput) {
  return runVisibilityChange(input, "unpark", "active")
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

/** Marks a completed active task as ready for human review. */
export async function runFinish(input: MutationInput & { task: string }) {
  const { context, state, diagnostics } = await readCommandState(input, "finish")
  const task = normalizeTaskName(input.task)
  const nextState = cloneState(state)
  const taskBranch =
    task === state.tasks.review
      ? state.branches.review
      : task === state.tasks.next
        ? state.branches.next
        : undefined
  const reviewReport = await readTaskReviewReport(context.rootDir, state.sprint, task, {
    ref: taskBranch,
  })
  const activeTasks = [state.tasks.review, state.tasks.next].filter(
    (activeTask): activeTask is string => Boolean(activeTask),
  )
  const alreadyFinished = state.tasks.finishedUnreviewed.includes(task)

  diagnostics.push(...reviewReport.diagnostics)
  if (!activeTasks.includes(task)) {
    diagnostics.push({
      severity: "error",
      code: "task_not_active",
      message: `Task ${task} is not recorded on review or next.`,
      suggestion: "Run sprint-branch start before finishing a task.",
    })
  }
  if (!alreadyFinished && state.tasks.finishedUnreviewed.length >= 2) {
    diagnostics.push({
      severity: "error",
      code: "unreviewed_limit_reached",
      message: "At most two tasks can be finished and unreviewed at once.",
      suggestion: "Run sprint-branch view and approve an existing finished task first.",
    })
  }

  if (!alreadyFinished) {
    nextState.tasks.finishedUnreviewed = [...nextState.tasks.finishedUnreviewed, task]
  }

  const plan = makePlan({
    command: "finish",
    context,
    state: nextState,
    summary: alreadyFinished
      ? `${task} is already marked finished-unreviewed.`
      : `Mark ${task} finished-unreviewed.`,
    requiresCleanWorkingTree: false,
    gitOperations: [],
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "Only Git-private sprint state is rewritten after the task Review Report is complete.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "finish", async () => {
    await writeSprintState(context.rootDir, nextState)
    return { ...plan, state: nextState, executed: true }
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
  const reviewReport = state.tasks.review
    ? await readTaskReviewReport(context.rootDir, state.sprint, state.tasks.review, {
        ref: state.branches.review,
      })
    : null

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
  if (state.tasks.review && !state.tasks.finishedUnreviewed.includes(state.tasks.review)) {
    diagnostics.push({
      severity: "error",
      code: "review_task_unfinished",
      message: `Review task ${state.tasks.review} is not marked finished-unreviewed.`,
      suggestion: `Run sprint-branch finish --task ${state.tasks.review} before approval.`,
    })
  }
  if (reviewReport) {
    diagnostics.push(...reviewReport.diagnostics)
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
    nextState.tasks.review = state.tasks.next
    nextState.tasks.next = null
  }
  nextState.tasks.finishedUnreviewed = nextState.tasks.finishedUnreviewed.filter(
    (task) => task !== state.tasks.review,
  )

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

/** Rebases the recorded sprint branch stack onto a target ref. */
export async function runRebase(input: MutationInput & { target: string }) {
  const { context, state, diagnostics } = await readCommandState(input, "rebase", {
    allowOwnConflictRetry: true,
  })
  const transientConflict = await readTransientConflict(context.rootDir, state.sprint)
  const retryConflict = rebaseConflict(state.conflict) ?? rebaseConflict(transientConflict)
  const retryingRebase = Boolean(retryConflict)
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const targetRef = input.target
  const targetExists = await refExists(context.rootDir, targetRef)
  const approvedHead = await getBranchHead(context.rootDir, state.branches.approved)
  const reviewHead = await getBranchHead(context.rootDir, state.branches.review)
  const nextHead = await getBranchHead(context.rootDir, state.branches.next)
  const approvedBase =
    readConflictString(retryConflict, "approvedBase") ??
    (targetExists && approvedHead
      ? await getMergeBase(context.rootDir, targetRef, approvedHead)
      : null)
  const originalApprovedHead = readConflictString(retryConflict, "approvedHead") ?? approvedHead
  const originalReviewHead = readConflictString(retryConflict, "reviewHead") ?? reviewHead
  const originalNextHead = readConflictString(retryConflict, "nextHead") ?? nextHead
  const nextState = cloneState(state)
  nextState.baseBranch = targetRef
  nextState.conflict = null

  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: "rebase requires a clean working tree.",
    })
  }
  if (state.activeStashes.length > 0) {
    diagnostics.push({
      severity: "error",
      code: "active_stashes_recorded",
      message: `State records ${state.activeStashes.length} active sprint stash record(s).`,
      suggestion: "Run sprint-branch resume before rebasing sprint branches.",
    })
  }
  if (Object.values(state.branches).includes(targetRef)) {
    diagnostics.push({
      severity: "error",
      code: "target_ref_is_sprint_branch",
      message: `Target ref ${targetRef} must not be one of the recorded sprint branches.`,
    })
  }
  if (!targetExists) {
    diagnostics.push({
      severity: "error",
      code: "target_ref_missing",
      message: `Target ref ${targetRef} does not resolve to a commit.`,
    })
  }
  if (targetExists && approvedHead && !approvedBase) {
    diagnostics.push({
      severity: "error",
      code: "target_ref_unrelated",
      message: `Target ref ${targetRef} has no merge base with ${state.branches.approved}.`,
    })
  }
  if (!retryingRebase) {
    await pushRebaseStackDiagnostics(
      context.rootDir,
      state,
      approvedHead,
      reviewHead,
      nextHead,
      diagnostics,
    )
  }

  const activeGitOperations = retryingRebase ? await getGitOperations(context.rootDir) : []
  if (activeGitOperations.length > 0) {
    await pushActiveGitOperationDiagnostics(context.rootDir, diagnostics)
  }

  const steps: Array<{ branch: string; onto: string; upstream: string }> = []
  if (approvedBase) {
    steps.push({
      branch: state.branches.approved,
      onto: targetRef,
      upstream: approvedBase,
    })
  }
  if (originalApprovedHead && reviewHead) {
    steps.push({
      branch: state.branches.review,
      onto: state.branches.approved,
      upstream: originalApprovedHead,
    })
  }
  if (nextHead && originalReviewHead) {
    steps.push({
      branch: state.branches.next,
      onto: state.branches.review,
      upstream: originalReviewHead,
    })
  }

  const retryStart = retryingRebase
    ? nextRebaseStepIndex(steps, readConflictString(retryConflict, "branch"))
    : 0
  if (retryingRebase && activeGitOperations.length === 0) {
    await pushCompletedRebaseDiagnostics(context.rootDir, steps.slice(0, retryStart), diagnostics)
  }

  const plannedSteps = steps.slice(retryStart)
  const gitOperations = plannedSteps.flatMap(formatRebaseStep)
  const finalPlannedBranch = plannedSteps[plannedSteps.length - 1]?.branch ?? null
  if (context.currentBranch && context.currentBranch !== finalPlannedBranch) {
    gitOperations.push(`git checkout ${context.currentBranch}`)
  }

  const plan = makePlan({
    command: "rebase",
    context,
    state: nextState,
    summary: `Rebase sprint ${state.sprint} branches onto ${targetRef}.`,
    requiresCleanWorkingTree: true,
    gitOperations,
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "Git performs each rebase. State keeps the previous base ref until every sprint branch is rebased; retry rebase after resolving any Git conflict.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "rebase", async () => {
    const metadata = {
      targetRef,
      currentBranch: context.currentBranch,
      approvedBase,
      approvedHead: originalApprovedHead,
      reviewHead: originalReviewHead,
      nextHead: originalNextHead,
    }
    let conflictBranch = plannedSteps[0]?.branch ?? context.currentBranch ?? state.branches.review

    try {
      for (const step of plannedSteps) {
        conflictBranch = step.branch
        await runGit(context.rootDir, ["checkout", step.branch])
        await runGit(context.rootDir, ["rebase", "--onto", step.onto, step.upstream])
      }
      if (context.currentBranch) {
        conflictBranch = context.currentBranch
        await runGit(context.rootDir, ["checkout", context.currentBranch])
      }

      await writeSprintState(context.rootDir, nextState)
      return { ...plan, state: nextState, executed: true }
    } catch (error) {
      if (error instanceof GitCommandError) {
        const conflictState = await writeConflictStateWhenSafe(
          context.rootDir,
          state,
          "rebase",
          conflictBranch,
          error,
          metadata,
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
  if (state.tasks.review || state.tasks.next) {
    diagnostics.push({
      severity: "error",
      code: "unreviewed_work_exists",
      message: "finalize requires no review task and no next task.",
    })
  }
  if (state.tasks.finishedUnreviewed.length > 0) {
    diagnostics.push({
      severity: "error",
      code: "unreviewed_work_exists",
      message: `finalize requires no finished-unreviewed tasks; found ${state.tasks.finishedUnreviewed.join(", ")}.`,
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
  if (!(await refExists(context.rootDir, baseBranch))) {
    diagnostics.push({
      severity: "error",
      code: "base_branch_missing",
      message: `Base ref ${baseBranch} does not resolve to a commit.`,
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

function rebaseConflict(conflict: SprintBranchState["conflict"] | null) {
  return conflict?.command === "rebase" ? conflict : null
}

function readConflictString(conflict: SprintBranchState["conflict"] | null, key: string) {
  const value = conflict?.[key]
  return typeof value === "string" && value.length > 0 ? value : null
}

function nextRebaseStepIndex(
  steps: Array<{ branch: string; onto: string; upstream: string }>,
  failedBranch: string | null,
) {
  const failedIndex = failedBranch ? steps.findIndex((step) => step.branch === failedBranch) : -1
  return failedIndex >= 0 ? failedIndex + 1 : 0
}

function formatRebaseStep(step: { branch: string; onto: string; upstream: string }) {
  return [`git checkout ${step.branch}`, `git rebase --onto ${step.onto} ${step.upstream}`]
}

async function pushRebaseStackDiagnostics(
  rootDir: string,
  state: SprintBranchState,
  approvedHead: string | null,
  reviewHead: string | null,
  nextHead: string | null,
  diagnostics: SprintDiagnostic[],
) {
  if (
    approvedHead &&
    reviewHead &&
    !(await isAncestor(rootDir, state.branches.approved, state.branches.review))
  ) {
    diagnostics.push({
      severity: "error",
      code: "review_not_based_on_approved",
      message: `${state.branches.review} does not descend from ${state.branches.approved}.`,
    })
  }
  if (
    nextHead &&
    reviewHead &&
    !(await isAncestor(rootDir, state.branches.review, state.branches.next))
  ) {
    diagnostics.push({
      severity: "error",
      code: "next_not_based_on_review",
      message: `${state.branches.next} does not descend from ${state.branches.review}.`,
    })
  }
}

async function pushCompletedRebaseDiagnostics(
  rootDir: string,
  completedSteps: Array<{ branch: string; onto: string; upstream: string }>,
  diagnostics: SprintDiagnostic[],
) {
  for (const step of completedSteps) {
    if (await isAncestor(rootDir, step.onto, step.branch)) {
      continue
    }

    diagnostics.push({
      severity: "error",
      code: "rebase_conflict_not_resolved",
      message: `${step.branch} does not descend from ${step.onto} after the recorded rebase conflict.`,
      suggestion: "Resolve or abort the Git rebase before retrying sprint-branch rebase.",
    })
  }
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

async function readResetSeedState(statePath: string, diagnostics: SprintDiagnostic[]) {
  try {
    const parsed = await readSprintStateFile(statePath)
    if (parsed.state) {
      return parsed.state
    }

    for (const diagnostic of parsed.diagnostics) {
      diagnostics.push({
        severity: "warning",
        code: `existing_state_${diagnostic.code}`,
        message: `Existing state is invalid and will be replaced: ${diagnostic.message}`,
      })
    }
    return null
  } catch (error) {
    if (isMissingFileError(error)) {
      diagnostics.push({
        severity: "info",
        code: "state_file_missing",
        message: "No existing sprint state file was found; reset-state will create one.",
      })
      return null
    }
    if (error instanceof SyntaxError) {
      diagnostics.push({
        severity: "warning",
        code: "existing_state_invalid_json",
        message: "Existing sprint state is not valid JSON and will be replaced.",
      })
      return null
    }
    throw error
  }
}

async function pushResetBranchDiagnostics(
  rootDir: string,
  branches: SprintBranchState["branches"],
  baseBranch: string,
  force: boolean,
  diagnostics: SprintDiagnostic[],
) {
  const approvedHead = await getBranchHead(rootDir, branches.approved)
  const reviewHead = await getBranchHead(rootDir, branches.review)
  const nextHead = await getBranchHead(rootDir, branches.next)

  if (!(await refExists(rootDir, baseBranch))) {
    diagnostics.push({
      severity: "error",
      code: "base_branch_missing",
      message: `Base ref ${baseBranch} does not resolve to a commit.`,
    })
  }
  if (!approvedHead) {
    diagnostics.push({
      severity: "error",
      code: "approved_branch_missing",
      message: `Approved branch ${branches.approved} does not exist.`,
    })
  }
  if (!reviewHead) {
    diagnostics.push({
      severity: "error",
      code: "review_branch_missing",
      message: `Review branch ${branches.review} does not exist.`,
    })
  }
  if (approvedHead && reviewHead && approvedHead !== reviewHead) {
    diagnostics.push({
      severity: force ? "warning" : "error",
      code: "review_branch_has_unrecorded_work",
      message: `${branches.review} differs from approved while reset-state would clear the review task mapping.`,
      suggestion: force
        ? "The next start may reset review back to approved."
        : "Use --force only after preserving or intentionally discarding review branch work.",
    })
  }
  if (reviewHead && nextHead && reviewHead !== nextHead) {
    diagnostics.push({
      severity: force ? "warning" : "error",
      code: "next_branch_has_unrecorded_work",
      message: `${branches.next} differs from review while reset-state would clear the next task mapping.`,
      suggestion: force
        ? "Doctor will continue to report unrecorded next-branch work until it is recovered or discarded."
        : "Use --force only after preserving or intentionally discarding next branch work.",
    })
  }
}

function pushResetStateDiagnostics(
  existingState: SprintBranchState | null,
  force: boolean,
  diagnostics: SprintDiagnostic[],
) {
  if (!existingState) {
    return
  }

  pushForceableResetDiagnostic(
    Boolean(existingState.conflict),
    force,
    diagnostics,
    "conflict_recorded",
    `State records an unresolved ${existingState.conflict?.command ?? "unknown"} conflict.`,
    "Resolve the Git conflict before resetting state, or use --force after preserving the work.",
  )
  pushForceableResetDiagnostic(
    existingState.activeStashes.length > 0,
    force,
    diagnostics,
    "active_stashes_recorded",
    `State records ${existingState.activeStashes.length} active sprint stash record(s).`,
    "Resume or inspect recorded stashes before resetting state, or use --force after preserving them.",
  )
  pushForceableResetDiagnostic(
    Boolean(existingState.tasks.review),
    force,
    diagnostics,
    "active_review_task_recorded",
    `State records review task ${existingState.tasks.review ?? "unknown"}.`,
    "Approve, recover, or preserve review work before resetting state, or use --force.",
  )
  pushForceableResetDiagnostic(
    Boolean(existingState.tasks.next),
    force,
    diagnostics,
    "active_next_task_recorded",
    `State records next task ${existingState.tasks.next ?? "unknown"}.`,
    "Resume, recover, or preserve next work before resetting state, or use --force.",
  )
}

function pushForceableResetDiagnostic(
  condition: boolean,
  force: boolean,
  diagnostics: SprintDiagnostic[],
  code: string,
  message: string,
  suggestion: string,
) {
  if (!condition) {
    return
  }

  diagnostics.push({
    severity: force ? "warning" : "error",
    code,
    message,
    suggestion,
  })
}

async function runVisibilityChange(
  input: MutationInput,
  command: "park" | "unpark",
  visibility: SprintBranchState["visibility"],
) {
  const { context, state, diagnostics } = await readCommandState(input, command)
  const nextState = { ...cloneState(state), visibility }
  const summary =
    state.visibility === visibility
      ? `Sprint ${state.sprint} is already ${visibility}.`
      : `${command === "park" ? "Park" : "Unpark"} sprint ${state.sprint}.`

  const plan = makePlan({
    command,
    context,
    state: nextState,
    summary,
    requiresCleanWorkingTree: false,
    gitOperations: [],
    stateFiles: stateFilesForState(nextState),
    conflictHandling:
      "Only Git-private sprint visibility is rewritten. Branches and working tree files are not moved.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, command, async () => {
    await writeSprintState(context.rootDir, nextState)
    return { ...plan, state: nextState, executed: true }
  })
}

function duplicateTaskPrefixDiagnostics(taskStems: string[]) {
  const diagnostics: SprintDiagnostic[] = []
  const prefixes = new Map<string, string[]>()

  for (const task of taskStems) {
    const match = task.match(/^(\d{3})-/)
    if (!match) {
      continue
    }
    prefixes.set(match[1], [...(prefixes.get(match[1]) ?? []), task])
  }

  for (const [prefix, tasks] of prefixes) {
    if (tasks.length > 1) {
      diagnostics.push({
        severity: "error",
        code: "duplicate_task_file_prefix",
        message: `Task prefix ${prefix} is used by multiple task files: ${tasks.join(", ")}.`,
      })
    }
  }

  return diagnostics
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

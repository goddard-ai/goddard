import * as fs from "node:fs/promises"
import path from "node:path"

import {
  branchExists,
  getBranchHead,
  getCurrentBranch,
  getGitOperations,
  getStashRefs,
  getWorkingTreeStatus,
  GitCommandError,
  isAncestor,
  resolveGitPath,
  runGit,
} from "./git"
import {
  getExpectedBranches,
  inferSprintContext,
  readSprintStateFile,
  sprintIndexPath,
  sprintStatePath,
} from "./state"
import type {
  SprintActiveStash,
  SprintBranchState,
  SprintConflictState,
  SprintContext,
  SprintDiagnostic,
  SprintMutationReport,
  SprintTaskState,
} from "./types"

const handoffFileName = "001-handoff.md"
const stateBlockStart = "<!-- sprint-branch-state:start -->"
const stateBlockEnd = "<!-- sprint-branch-state:end -->"

type MutationInput = {
  cwd: string
  sprint?: string
  dryRun: boolean
}

type MutationPlan = {
  command: string
  context: SprintContext
  state: SprintBranchState
  summary: string
  requiresCleanWorkingTree: boolean
  gitOperations: string[]
  sprintFiles: string[]
  conflictHandling: string
  diagnostics: SprintDiagnostic[]
}

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
    sprintFiles: sprintFilesForState(context.rootDir, state),
    conflictHandling: "No rebase is performed. Existing branches or state files stop the command.",
    diagnostics,
  })

  if (input.dryRun || !plan.ok) {
    return withDryRun(plan, input.dryRun)
  }

  return withSprintLock(context, state, "init", async () => {
    await runGit(context.rootDir, ["branch", branches.approved, input.base])
    await runGit(context.rootDir, ["branch", branches.review, branches.approved])
    await writeSprintFiles(context.rootDir, state, "init", `Initialized sprint from ${input.base}.`)
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
    sprintFiles: sprintFilesForState(context.rootDir, nextState),
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
    await writeSprintFiles(context.rootDir, nextState, "start", summary)
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
    sprintFiles: sprintFilesForState(context.rootDir, nextState),
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
    await writeSprintFiles(
      context.rootDir,
      nextState,
      "feedback",
      recordedStash
        ? `Stashed ${state.tasks.next} from next and checked out review.`
        : "Checked out review for feedback.",
    )
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
    sprintFiles: sprintFilesForState(context.rootDir, nextState),
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
        await writeSprintFiles(context.rootDir, nextState, "resume", plan.summary)
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
      await writeSprintFiles(context.rootDir, nextState, "resume", plan.summary)
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
    sprintFiles: sprintFilesForState(context.rootDir, nextState),
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
      await writeSprintFiles(context.rootDir, nextState, "approve", plan.summary)
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
    sprintFiles: sprintFilesForState(context.rootDir, nextState),
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
      await writeSprintFiles(context.rootDir, nextState, "finalize", plan.summary)
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

/** Formats a mutation report for human-oriented CLI output. */
export function formatMutationReport(report: SprintMutationReport) {
  const lines = [
    `${report.dryRun ? "Dry run" : report.executed ? "Executed" : "Planned"}: ${report.command}`,
    `Sprint: ${report.sprint}`,
    `Current branch: ${report.currentBranch ?? "detached HEAD"}`,
    `Summary: ${report.summary}`,
    `Working tree must be clean: ${report.requiresCleanWorkingTree ? "yes" : "no"}`,
    "",
    "Git operations:",
    ...formatList(report.gitOperations),
    "",
    "Sprint files:",
    ...formatList(report.sprintFiles),
    "",
    `Conflict handling: ${report.conflictHandling}`,
  ]

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

async function readCommandState(
  input: MutationInput,
  commandName: string,
  options: { allowOwnConflictRetry?: boolean } = {},
) {
  const context = await inferSprintContext(input)
  const parsed = await readSprintStateFile(context.statePath)
  const diagnostics = [...parsed.diagnostics]

  if (!parsed.state) {
    throw new SprintMutationError({
      ok: false,
      command: commandName,
      dryRun: input.dryRun,
      executed: false,
      sprint: context.sprint,
      currentBranch: context.currentBranch,
      summary: "Sprint state is invalid.",
      requiresCleanWorkingTree: true,
      gitOperations: [],
      sprintFiles: [context.stateRelativePath],
      conflictHandling: "Fix the JSON state before running mutating commands.",
      diagnostics,
      state: null,
    })
  }
  if (
    parsed.state.conflict &&
    !(options.allowOwnConflictRetry && parsed.state.conflict.command === commandName)
  ) {
    diagnostics.push({
      severity: "error",
      code: "conflict_recorded",
      message: `State records an unresolved ${parsed.state.conflict.command ?? "unknown"} conflict.`,
      suggestion: "Resolve the Git conflict and inspect sprint-branch doctor before continuing.",
    })
  } else if (parsed.state.conflict) {
    diagnostics.push({
      severity: "warning",
      code: "retrying_recorded_conflict",
      message: `Retrying ${commandName} after a recorded conflict.`,
      suggestion: "Finish any active Git operation before retrying the command.",
    })
  }
  if (parsed.state.lock) {
    diagnostics.push({
      severity: "error",
      code: "state_lock_recorded",
      message: `State records an active ${parsed.state.lock.command} lock.`,
      suggestion: "Run sprint-branch doctor before retrying the command.",
    })
  }
  if (!(await branchExists(context.rootDir, parsed.state.branches.approved))) {
    diagnostics.push({
      severity: "error",
      code: "approved_branch_missing",
      message: `Approved branch ${parsed.state.branches.approved} does not exist.`,
    })
  }
  if (!(await branchExists(context.rootDir, parsed.state.branches.review))) {
    diagnostics.push({
      severity: "error",
      code: "review_branch_missing",
      message: `Review branch ${parsed.state.branches.review} does not exist.`,
    })
  }
  if (
    parsed.state.tasks.next &&
    !(await branchExists(context.rootDir, parsed.state.branches.next))
  ) {
    diagnostics.push({
      severity: "error",
      code: "next_branch_missing",
      message: `Next branch ${parsed.state.branches.next} does not exist.`,
    })
  }

  return { context, state: parsed.state, diagnostics }
}

export class SprintMutationError extends Error {
  report: SprintMutationReport

  constructor(report: SprintMutationReport) {
    super(report.summary)
    this.name = "SprintMutationError"
    this.report = report
  }
}

function makePlan(input: Omit<MutationPlan, "diagnostics"> & { diagnostics: SprintDiagnostic[] }) {
  return {
    ok: !input.diagnostics.some((diagnostic) => diagnostic.severity === "error"),
    command: input.command,
    dryRun: false,
    executed: false,
    sprint: input.state.sprint,
    currentBranch: input.context.currentBranch,
    summary: input.summary,
    requiresCleanWorkingTree: input.requiresCleanWorkingTree,
    gitOperations: input.gitOperations,
    sprintFiles: input.sprintFiles,
    conflictHandling: input.conflictHandling,
    diagnostics: input.diagnostics,
    state: input.state,
  } satisfies SprintMutationReport
}

function withDryRun(report: SprintMutationReport, dryRun: boolean) {
  return dryRun ? { ...report, dryRun } : report
}

async function withSprintLock(
  context: SprintContext,
  state: SprintBranchState,
  commandName: string,
  run: () => Promise<SprintMutationReport>,
) {
  const lockPath = await resolveGitPath(context.rootDir, `sprint-branch/${context.sprint}.lock`)
  const lock = {
    command: commandName,
    createdAt: new Date().toISOString(),
    pid: process.pid,
  }
  let handle: fs.FileHandle | null = null

  try {
    await fs.mkdir(path.dirname(lockPath), { recursive: true })
    handle = await fs.open(lockPath, "wx")
    await handle.writeFile(`${JSON.stringify(lock, null, 2)}\n`)
    await handle.close()
    handle = null
    return await run()
  } catch (error) {
    if (isAlreadyExistsError(error)) {
      return {
        ok: false,
        command: commandName,
        dryRun: false,
        executed: false,
        sprint: state.sprint,
        currentBranch: context.currentBranch,
        summary: `Sprint ${state.sprint} is locked by another branch operation.`,
        requiresCleanWorkingTree: true,
        gitOperations: [],
        sprintFiles: [path.relative(context.rootDir, lockPath)],
        conflictHandling:
          "Remove the lock only after confirming no sprint-branch command is running.",
        diagnostics: [
          {
            severity: "error",
            code: "lock_exists",
            message: `Lock file ${path.relative(context.rootDir, lockPath)} already exists.`,
          },
        ],
        state,
      } satisfies SprintMutationReport
    }
    throw error
  } finally {
    if (handle) {
      await handle.close()
    }
    await fs.rm(lockPath, { force: true })
  }
}

async function writeSprintFiles(
  rootDir: string,
  state: SprintBranchState,
  commandName: string,
  note: string,
) {
  await fs.mkdir(path.join(rootDir, "sprints", state.sprint), { recursive: true })
  await writeSprintStateAtomic(sprintStatePath(rootDir, state.sprint), {
    ...state,
    lock: null,
  })
  await upsertIndexMirror(rootDir, state)
  await appendHandoff(rootDir, state, commandName, note)
  await clearTransientConflict(rootDir, state.sprint)
}

async function writeConflictStateWhenSafe(
  rootDir: string,
  state: SprintBranchState,
  commandName: string,
  branch: string,
  error: GitCommandError,
  metadata: Record<string, unknown> = {},
) {
  const conflictState = makeConflictState(state, commandName, branch, error, metadata)
  const operations = await getGitOperations(rootDir)
  if (operations.length > 0) {
    await writeTransientConflict(rootDir, state.sprint, conflictState.conflict)
    return conflictState
  }

  await writeSprintFiles(
    rootDir,
    conflictState,
    commandName,
    `Stopped on conflict while running ${commandName} on ${branch}.`,
  )
  return conflictState
}

async function readTransientConflict(rootDir: string, sprint: string) {
  try {
    return JSON.parse(
      await fs.readFile(await transientConflictPath(rootDir, sprint), "utf-8"),
    ) as SprintConflictState
  } catch (error) {
    if (isMissingFileError(error) || error instanceof SyntaxError) {
      return null
    }
    throw error
  }
}

async function writeTransientConflict(
  rootDir: string,
  sprint: string,
  conflict: SprintConflictState | null,
) {
  if (!conflict) {
    return
  }

  const markerPath = await transientConflictPath(rootDir, sprint)
  await fs.mkdir(path.dirname(markerPath), { recursive: true })
  await fs.writeFile(markerPath, `${JSON.stringify(conflict, null, 2)}\n`)
}

async function clearTransientConflict(rootDir: string, sprint: string) {
  await fs.rm(await transientConflictPath(rootDir, sprint), { force: true })
}

async function transientConflictPath(rootDir: string, sprint: string) {
  return resolveGitPath(rootDir, `sprint-branch/${sprint}.conflict.json`)
}

function makeConflictState(
  state: SprintBranchState,
  commandName: string,
  branch: string,
  error: GitCommandError,
  metadata: Record<string, unknown>,
) {
  return {
    ...state,
    lock: null,
    conflict: {
      command: commandName,
      branch,
      message: error.stderr || error.message,
      ...metadata,
    },
  }
}

function conflictReport(
  plan: SprintMutationReport,
  state: SprintBranchState,
  error: GitCommandError,
) {
  return {
    ...plan,
    ok: false,
    executed: true,
    state,
    diagnostics: [
      ...plan.diagnostics,
      {
        severity: "error" as const,
        code: "git_operation_failed",
        message: error.stderr || error.message,
      },
    ],
  }
}

async function writeSprintStateAtomic(statePath: string, state: SprintBranchState) {
  await fs.mkdir(path.dirname(statePath), { recursive: true })
  const tempPath = `${statePath}.${process.pid}.${Date.now()}.tmp`
  await fs.writeFile(tempPath, `${JSON.stringify(state, null, 2)}\n`)
  await fs.rename(tempPath, statePath)
}

async function upsertIndexMirror(rootDir: string, state: SprintBranchState) {
  const indexPath = sprintIndexPath(rootDir, state.sprint)
  const block = renderIndexBlock(state)
  let existing = ""

  try {
    existing = await fs.readFile(indexPath, "utf-8")
  } catch (error) {
    if (!isMissingFileError(error)) {
      throw error
    }
  }

  let nextText = ""
  const start = existing.indexOf(stateBlockStart)
  const end = existing.indexOf(stateBlockEnd)

  if (start !== -1 && end !== -1 && end > start) {
    nextText = `${existing.slice(0, start)}${block}${existing.slice(end + stateBlockEnd.length)}`
  } else if (existing.trim().length > 0) {
    nextText = `${block}\n\n${existing}`
  } else {
    nextText = `# Sprint ${state.sprint}\n\n${block}\n`
  }

  await fs.writeFile(indexPath, ensureTrailingNewline(nextText))
}

async function appendHandoff(
  rootDir: string,
  state: SprintBranchState,
  commandName: string,
  note: string,
) {
  const handoffPath = path.join(rootDir, "sprints", state.sprint, handoffFileName)
  const header = (await pathExists(handoffPath)) ? "" : `# Sprint ${state.sprint} Handoff\n\n`
  const entry = [
    `## ${new Date().toISOString()} sprint-branch ${commandName}`,
    "",
    `- ${note}`,
    `- Review: ${state.branches.review} (${state.tasks.review ?? "no task"})`,
    `- Next: ${state.branches.next} (${state.tasks.next ?? "no task"})`,
    `- Approved: ${state.tasks.approved.length ? state.tasks.approved.join(", ") : "none"}`,
    "",
  ].join("\n")

  await fs.appendFile(handoffPath, `${header}${entry}`)
}

function renderIndexBlock(state: SprintBranchState) {
  return [
    stateBlockStart,
    "## Sprint Branch State",
    "",
    `- Sprint: ${state.sprint}`,
    `- Base branch: ${state.baseBranch}`,
    `- Review branch: ${state.branches.review}`,
    `- Approved branch: ${state.branches.approved}`,
    `- Next branch: ${state.branches.next}`,
    `- Review task: ${state.tasks.review ?? "none"}`,
    `- Next task: ${state.tasks.next ?? "none"}`,
    `- Approved tasks: ${state.tasks.approved.length ? state.tasks.approved.join(", ") : "none"}`,
    `- Finished unreviewed: ${
      state.tasks.finishedUnreviewed.length ? state.tasks.finishedUnreviewed.join(", ") : "none"
    }`,
    `- Active stashes: ${
      state.activeStashes.length
        ? state.activeStashes.map((stash) => stash.ref ?? stash.message ?? "unknown").join(", ")
        : "none"
    }`,
    `- Blocked: ${state.conflict ? `conflict in ${state.conflict.command ?? "unknown"}` : "no"}`,
    `- Next safe command: ${nextSafeCommandForState(state)}`,
    stateBlockEnd,
  ].join("\n")
}

function nextSafeCommandForState(state: SprintBranchState) {
  if (state.conflict) {
    return "sprint-branch doctor"
  }
  if (state.tasks.review) {
    return "sprint-branch approve --dry-run"
  }
  if (state.tasks.next) {
    return "sprint-branch resume --dry-run"
  }
  return "sprint-branch start --task <task-file> --dry-run"
}

async function resolveNextPlannedTask(rootDir: string, state: SprintBranchState) {
  const tasks = await listTaskStems(rootDir, state.sprint)
  const assigned = new Set([
    ...state.tasks.approved,
    ...state.tasks.finishedUnreviewed,
    state.tasks.review,
    state.tasks.next,
  ])
  return tasks.find((task) => !assigned.has(task)) ?? null
}

async function listTaskStems(rootDir: string, sprint: string) {
  const sprintDir = path.join(rootDir, "sprints", sprint)
  const entries = await fs.readdir(sprintDir, { withFileTypes: true })
  return entries
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .filter((name) => name.endsWith(".md"))
    .filter((name) => name !== "000-index.md" && name !== handoffFileName)
    .map((name) => name.slice(0, -".md".length))
    .sort()
}

async function taskFileExists(rootDir: string, sprint: string, task: string) {
  return pathExists(path.join(rootDir, "sprints", sprint, `${task}.md`))
}

function normalizeTaskName(task: string) {
  const normalized = task.endsWith(".md") ? task.slice(0, -".md".length) : task
  if (normalized.includes("/") || normalized.includes("\\") || normalized.length === 0) {
    throw new Error("Task must be a task file stem inside the sprint folder.")
  }
  return normalized
}

function nextTaskDiagnostic(task: string, plannedTask: string) {
  return {
    severity: "error" as const,
    code: "task_out_of_order",
    message: `${task} is not the next planned task; expected ${plannedTask}.`,
  }
}

function noPlannedTaskDiagnostic(task: string) {
  return {
    severity: "error" as const,
    code: "no_planned_task",
    message: `${task} is not available because no unassigned sprint tasks remain.`,
  }
}

async function pushActiveGitOperationDiagnostics(rootDir: string, diagnostics: SprintDiagnostic[]) {
  const operations = await getGitOperations(rootDir)
  for (const operation of operations) {
    diagnostics.push({
      severity: "error",
      code: "git_operation_in_progress",
      message: `Git ${operation.name} operation is still in progress.`,
      suggestion: "Resolve it with Git before retrying the sprint-branch command.",
    })
  }
}

function isRetryingCommand(state: SprintBranchState, commandName: string) {
  return state.conflict?.command === commandName
}

function hasUnmergedEntries(entries: string[]) {
  return entries.some((entry) =>
    ["DD", "AU", "UD", "UA", "DU", "AA", "UU"].includes(entry.slice(0, 2)),
  )
}

async function moveRecordedBranch(
  rootDir: string,
  state: SprintBranchState,
  branch: string,
  target: string,
) {
  if (!Object.values(state.branches).includes(branch)) {
    throw new Error(`Refusing to move unrecorded branch ${branch}.`)
  }

  if ((await getCurrentBranch(rootDir)) === branch) {
    await runGit(rootDir, ["reset", "--hard", target])
    return
  }

  await runGit(rootDir, ["branch", "--force", branch, target])
}

function moveBranchOperation(branch: string, target: string, currentBranch: string | null) {
  return currentBranch === branch
    ? `git reset --hard ${target}`
    : `git branch --force ${branch} ${target}`
}

function findMatchingStash(state: SprintBranchState) {
  return state.activeStashes.find(
    (stash) =>
      stash.sourceBranch === state.branches.next &&
      stash.task === state.tasks.next &&
      stash.reason === "feedback",
  )
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

function emptyTasks(): SprintTaskState {
  return {
    review: null,
    next: null,
    approved: [],
    finishedUnreviewed: [],
  }
}

function cloneState(state: SprintBranchState): SprintBranchState {
  return JSON.parse(JSON.stringify(state)) as SprintBranchState
}

function sprintFilesForState(rootDir: string, state: SprintBranchState) {
  return [
    path.relative(rootDir, sprintStatePath(rootDir, state.sprint)),
    path.relative(rootDir, sprintIndexPath(rootDir, state.sprint)),
    path.join("sprints", state.sprint, handoffFileName),
  ]
}

function formatList(values: string[]) {
  if (values.length === 0) {
    return ["  none"]
  }
  return values.map((value) => `  - ${value}`)
}

function ensureTrailingNewline(value: string) {
  return value.endsWith("\n") ? value : `${value}\n`
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

function isAlreadyExistsError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "EEXIST"
  )
}

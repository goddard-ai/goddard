import { branchExists } from "../git/refs"
import { inferSprintContext } from "../state/inference"
import { readSprintStateFile } from "../state/io"
import { SprintMutationError, type MutationInput } from "./report"

/** Reads canonical state and validates common preconditions for mutating commands. */
export async function readCommandState(
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
      stateFiles: [context.stateRelativePath],
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

import { GitCommandError } from "../git/command"
import { getGitOperations } from "../git/repository"
import { writeTransientConflict } from "../transient-conflict"
import type { SprintBranchState, SprintDiagnostic, SprintMutationReport } from "../types"
import { writeSprintFiles } from "./sprint-files"

/** Writes conflict metadata in the safest location for the current Git operation state. */
export async function writeConflictStateWhenSafe(
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

/** Converts a failed Git transition into the common mutation report shape. */
export function conflictReport(
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

/** Adds diagnostics for in-progress Git operations that must be completed manually. */
export async function pushActiveGitOperationDiagnostics(
  rootDir: string,
  diagnostics: SprintDiagnostic[],
) {
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

/** Checks whether a command is retrying its own recorded conflict. */
export function isRetryingCommand(state: SprintBranchState, commandName: string) {
  return state.conflict?.command === commandName
}

/** Detects unresolved index conflicts from porcelain status entries. */
export function hasUnmergedEntries(entries: string[]) {
  return entries.some((entry) =>
    ["DD", "AU", "UD", "UA", "DU", "AA", "UU"].includes(entry.slice(0, 2)),
  )
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

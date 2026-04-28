import type {
  SprintBranchState,
  SprintContext,
  SprintDiagnostic,
  SprintMutationReport,
} from "../types"

/** Common input shared by mutating sprint workflow commands. */
export type MutationInput = {
  cwd: string
  sprint?: string
  dryRun: boolean
}

/** Internal execution plan for one mutating sprint workflow command. */
export type MutationPlan = {
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

/** Error raised when a mutating command cannot produce a valid command state. */
export class SprintMutationError extends Error {
  report: SprintMutationReport

  constructor(report: SprintMutationReport) {
    super(report.summary)
    this.name = "SprintMutationError"
    this.report = report
  }
}

/** Builds a mutation report from a validated transition plan. */
export function makePlan(
  input: Omit<MutationPlan, "diagnostics"> & { diagnostics: SprintDiagnostic[] },
) {
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

/** Marks a mutation report as dry-run output without changing the planned operation. */
export function withDryRun(report: SprintMutationReport, dryRun: boolean) {
  return dryRun ? { ...report, dryRun } : report
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

function formatList(values: string[]) {
  if (values.length === 0) {
    return ["  none"]
  }
  return values.map((value) => `  - ${value}`)
}

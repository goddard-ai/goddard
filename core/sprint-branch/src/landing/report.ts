import { GitCommandError } from "../git/command"
import type { SprintCleanupReport, SprintLandReport } from "./types"

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

/** Converts a failed human Git operation into a structured landing report. */
export function handleHumanGitError<T extends SprintLandReport | SprintCleanupReport>(
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

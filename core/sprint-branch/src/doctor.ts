import {
  dedupeDiagnostics,
  dedupeStrings,
  filterStatusDiagnostics,
  resolveDoctorNextSafeCommand,
  runDoctorChecks,
} from "./doctor/checks"
import type { DoctorContext } from "./doctor/types"
import { getGitOperations } from "./git/repository"
import { buildStatusReport } from "./status"
import { readTransientConflict } from "./transient-conflict"
import type { SprintStatusReport } from "./types"

/** Builds the deeper consistency report for sprint-branch doctor. */
export async function buildDoctorReport(input: { cwd: string; sprint?: string }) {
  const { context, report, diagnostics } = await buildStatusReport(input)
  if (!report) {
    return { context, report, diagnostics }
  }

  const doctorContext = {
    transientConflict: await readTransientConflict(report.rootDir, report.state.sprint),
    gitOperations: await getGitOperations(report.rootDir),
  } satisfies DoctorContext
  const doctorDiagnostics = dedupeDiagnostics([
    ...filterStatusDiagnostics(report, doctorContext),
    ...(await runDoctorChecks(report, doctorContext)),
  ])
  const doctorReport: SprintStatusReport = {
    ...report,
    ok: !doctorDiagnostics.some((diagnostic) => diagnostic.severity === "error"),
    diagnostics: doctorDiagnostics,
    blocked: {
      ...report.blocked,
      conflict: report.blocked.conflict || Boolean(doctorContext.transientConflict),
      reasons: dedupeStrings([
        ...report.blocked.reasons,
        ...(doctorContext.transientConflict ? ["transition retry pending"] : []),
      ]),
      nextSafeCommand: resolveDoctorNextSafeCommand(report, doctorContext),
    },
  }

  return {
    context,
    report: doctorReport,
    diagnostics: doctorDiagnostics,
  }
}

/** Formats doctor diagnostics with a concise recovery suggestion. */
export function formatDoctorReport(report: SprintStatusReport) {
  if (report.diagnostics.length === 0) {
    return [
      `No sprint branch issues found for ${report.sprint}.`,
      `Next safe command: ${report.blocked.nextSafeCommand ?? "sprint-branch status"}`,
    ].join("\n")
  }

  const lines = [`Sprint branch issues for ${report.sprint}:`]
  for (const diagnostic of report.diagnostics) {
    lines.push(`  [${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`)
    if (diagnostic.suggestion) {
      lines.push(`    suggestion: ${diagnostic.suggestion}`)
    }
  }

  lines.push(`Next safe command: ${report.blocked.nextSafeCommand ?? "manual recovery required"}`)
  return lines.join("\n")
}

import { watchReviewSession, type ReviewSyncResult } from "@goddard-ai/review-sync"

import { SprintInferenceError } from "./state/inference"
import { buildStatusReport } from "./status"
import type { SprintDiagnostic, SprintSyncReport } from "./types"

/** Resolves the active sprint and watches review-sync for its review branch. */
export async function runSprintSync(input: {
  cwd: string
  sprint?: string
  interactive?: boolean
  signal?: AbortSignal
  onResult?: (result: ReviewSyncResult) => void | Promise<void>
}) {
  let status: Awaited<ReturnType<typeof buildStatusReport>>
  try {
    status = await buildStatusReport(input)
  } catch (error) {
    if (error instanceof SprintInferenceError) {
      return {
        ok: false,
        command: "sync",
        sprint: null,
        currentBranch: null,
        inferredFrom: null,
        agentBranch: null,
        reviewBranch: null,
        diagnostics: inferenceDiagnostics(error),
        reviewSync: null,
      } satisfies SprintSyncReport
    }
    throw error
  }

  const { report, diagnostics } = status

  if (!report) {
    return {
      ok: false,
      command: "sync",
      sprint: null,
      currentBranch: null,
      inferredFrom: null,
      agentBranch: null,
      reviewBranch: null,
      diagnostics,
      reviewSync: null,
    } satisfies SprintSyncReport
  }

  if (!report.ok) {
    return {
      ok: false,
      command: "sync",
      sprint: report.sprint,
      currentBranch: report.currentBranch,
      inferredFrom: report.inferredFrom,
      agentBranch: report.state.branches.review,
      reviewBranch: null,
      diagnostics: report.diagnostics,
      reviewSync: null,
    } satisfies SprintSyncReport
  }

  const reviewSync = await watchReviewSession({
    cwd: input.cwd,
    agentBranch: report.state.branches.review,
    signal: input.signal,
    onResult: input.onResult,
  })

  return {
    ok: reviewSync.exitCode === 0,
    command: "sync",
    sprint: report.sprint,
    currentBranch: report.currentBranch,
    inferredFrom: report.inferredFrom,
    agentBranch: report.state.branches.review,
    reviewBranch: reviewSync.reviewBranch ?? null,
    diagnostics: report.diagnostics,
    reviewSync,
  } satisfies SprintSyncReport
}

/** Formats the sprint sync wrapper without hiding review-sync's own message. */
export function formatSprintSyncReport(report: SprintSyncReport) {
  const lines = []

  if (!report.reviewSync && report.sprint) {
    lines.push(`Sprint: ${report.sprint}`)
  }
  if (!report.reviewSync && report.agentBranch) {
    lines.push(`Agent branch: ${report.agentBranch}`)
  }
  if (!report.reviewSync && report.reviewBranch) {
    lines.push(`Review branch: ${report.reviewBranch}`)
  }

  if (report.reviewSync?.message) {
    if (lines.length > 0) {
      lines.push("")
    }
    lines.push(report.reviewSync.message)
  }

  if (report.diagnostics.length > 0) {
    if (lines.length > 0) {
      lines.push("")
    }
    lines.push(...formatDiagnostics(report.diagnostics))
  }

  if (lines.length === 0) {
    return report.ok ? "Sync complete." : "Sync failed."
  }

  return lines.join("\n")
}

function formatDiagnostics(diagnostics: SprintDiagnostic[]) {
  return diagnostics.flatMap((diagnostic) => [
    `[${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`,
    ...(diagnostic.suggestion ? [`suggestion: ${diagnostic.suggestion}`] : []),
  ])
}

function inferenceDiagnostics(error: SprintInferenceError) {
  if (error.diagnostics.length > 0) {
    return error.diagnostics
  }

  return [
    {
      severity: "error" as const,
      code: "sprint_inference_failed",
      message: error.message,
    },
  ]
}

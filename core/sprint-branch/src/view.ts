import { readTaskReviewReport } from "./review-report"
import { buildStatusReport } from "./status"
import type { SprintDiagnostic, SprintReviewViewReport, SprintStatusReport } from "./types"
import { normalizeTaskName } from "./workflow/tasks"

/** Builds the concise human approval view for the current sprint review task. */
export async function buildSprintReviewView(input: {
  cwd: string
  sprint?: string
  task?: string
  interactive?: boolean
}) {
  const { report: status, diagnostics } = await buildStatusReport(input)
  if (!status) {
    return {
      report: null,
      diagnostics,
    }
  }

  const viewDiagnostics: SprintDiagnostic[] = [...status.diagnostics]
  const task = input.task ? normalizeTaskName(input.task) : status.state.tasks.review
  const diffCommand = `sprint-branch diff --sprint ${status.sprint} --stat`

  if (!task) {
    viewDiagnostics.push({
      severity: "error",
      code: "review_task_missing",
      message: "No current review task is recorded.",
      suggestion: "Run sprint-branch start --task <task-id> before asking for review.",
    })
    return {
      report: {
        ok: false,
        sprint: status.sprint,
        task: null,
        reviewBranch: status.state.branches.review,
        approvedBranch: status.state.branches.approved,
        diffCommand,
        reviewReport: null,
        diagnostics: viewDiagnostics,
      } satisfies SprintReviewViewReport,
      diagnostics: viewDiagnostics,
    }
  }

  const taskReviewReport = await readTaskReviewReport(status.rootDir, status.sprint, task, {
    ref: taskBranch(status, task),
  })
  viewDiagnostics.push(...taskReviewReport.diagnostics)
  if (!status.state.tasks.finishedUnreviewed.includes(task)) {
    viewDiagnostics.push({
      severity: "error",
      code: "task_not_finished_unreviewed",
      message: `Task ${task} is not marked finished-unreviewed in sprint state.`,
      suggestion: `Run sprint-branch finish --task ${task}.`,
    })
  }

  const report = {
    ok: !viewDiagnostics.some((diagnostic) => diagnostic.severity === "error"),
    sprint: status.sprint,
    task: {
      id: task,
      title: taskReviewReport.taskTitle,
      path: taskReviewReport.taskPath,
      state: resolveTaskState(status, task),
    },
    reviewBranch: status.state.branches.review,
    approvedBranch: status.state.branches.approved,
    diffCommand,
    reviewReport: taskReviewReport.ok ? taskReviewReport.reviewReport : null,
    diagnostics: viewDiagnostics,
  } satisfies SprintReviewViewReport

  return {
    report,
    diagnostics: viewDiagnostics,
  }
}

/** Formats the sprint review view with the Review Report as the main content. */
export function formatSprintReviewView(report: SprintReviewViewReport) {
  if (!report.ok) {
    const lines = ["Cannot build sprint review view."]
    for (const diagnostic of report.diagnostics) {
      lines.push(`  [${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`)
      if (diagnostic.suggestion) {
        lines.push(`    suggestion: ${diagnostic.suggestion}`)
      }
    }
    return lines.join("\n")
  }

  const task = report.task
  return [
    `Task: ${task ? `${task.id} - ${task.title}` : "none"}`,
    `Review branch: ${report.reviewBranch}`,
    `Approved comparison branch: ${report.approvedBranch}`,
    `Diff: ${report.diffCommand}`,
    "",
    report.reviewReport ?? "",
  ].join("\n")
}

function resolveTaskState(status: SprintStatusReport, task: string) {
  if (status.state.tasks.finishedUnreviewed.includes(task)) {
    return "finished-unreviewed"
  }
  if (status.state.tasks.review === task) {
    return "review"
  }
  if (status.state.tasks.next === task) {
    return "next"
  }
  if (status.state.tasks.approved.includes(task)) {
    return "approved"
  }
  if (status.taskQueue.some((item) => item.id === task)) {
    return "planned"
  }
  return "unknown"
}

function taskBranch(status: SprintStatusReport, task: string) {
  if (status.state.tasks.next === task) {
    return status.state.branches.next
  }
  return status.state.branches.review
}

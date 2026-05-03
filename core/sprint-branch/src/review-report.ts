import * as fs from "node:fs/promises"
import path from "node:path"

import { GitCommandError, runGit } from "./git/command"
import type { SprintDiagnostic } from "./types"

/** Required task Review Report subsections, in the order humans review them. */
export const reviewReportHeadings = [
  "Plain-English Summary",
  "How To Verify Without Reading Code",
  "Agent Verification",
  "Approval Questions",
  "Known Limits",
]

/** Parsed task markdown and Review Report validation details. */
export type TaskReviewReport = {
  ok: boolean
  task: string
  taskTitle: string
  taskPath: string
  reviewReport: string | null
  diagnostics: SprintDiagnostic[]
}

/** Reads a sprint task file and validates its required Review Report section. */
export async function readTaskReviewReport(
  rootDir: string,
  sprint: string,
  task: string,
  options: { ref?: string } = {},
) {
  const taskPath = path.join("sprints", sprint, `${task}.md`)
  let text = ""

  try {
    text = options.ref
      ? await runGit(rootDir, ["show", `${options.ref}:${taskPath}`])
      : await fs.readFile(path.join(rootDir, taskPath), "utf-8")
  } catch (error) {
    if (isMissingFileError(error) || error instanceof GitCommandError) {
      const diagnostics = [
        {
          severity: "error" as const,
          code: "task_file_missing",
          message: options.ref
            ? `Task file ${taskPath} does not exist on ${options.ref}.`
            : `Task file ${taskPath} does not exist.`,
        },
      ]
      return {
        ok: false,
        task,
        taskTitle: task,
        taskPath,
        reviewReport: null,
        diagnostics,
      } satisfies TaskReviewReport
    }
    throw error
  }

  const parsed = parseReviewReport(text, task)
  return {
    ok: parsed.diagnostics.length === 0,
    task,
    taskTitle: readTaskTitle(text) ?? task,
    taskPath,
    reviewReport: parsed.reviewReport,
    diagnostics: parsed.diagnostics,
  } satisfies TaskReviewReport
}

/** Extracts and validates the Review Report block from one task markdown document. */
export function parseReviewReport(text: string, task: string) {
  const lines = text.split(/\r?\n/)
  const reportStart = lines.findIndex((line) => {
    const heading = parseHeading(line)
    return heading?.level === 2 && heading.text === "Review Report"
  })

  if (reportStart === -1) {
    return {
      reviewReport: null,
      diagnostics: [
        {
          severity: "error" as const,
          code: "review_report_missing",
          message: `Task ${task} is missing a ## Review Report section.`,
        },
      ],
    }
  }

  const reportEnd = findNextHeadingAtOrAbove(lines, reportStart + 1, 2)
  const reportLines = lines.slice(reportStart, reportEnd === -1 ? lines.length : reportEnd)
  const diagnostics: SprintDiagnostic[] = []
  let cursor = 1

  for (const heading of reviewReportHeadings) {
    const index = reportLines.findIndex((line, lineIndex) => {
      if (lineIndex < cursor) {
        return false
      }
      const parsed = parseHeading(line)
      return parsed?.level === 3 && parsed.text === heading
    })

    if (index === -1) {
      diagnostics.push({
        severity: "error",
        code: "review_report_incomplete",
        message: `Task ${task} Review Report is missing ### ${heading}.`,
      })
      continue
    }

    const sectionEnd = findNextHeadingAtOrAbove(reportLines, index + 1, 3)
    const body = reportLines
      .slice(index + 1, sectionEnd === -1 ? reportLines.length : sectionEnd)
      .join("\n")
      .trim()
    if (body.length === 0) {
      diagnostics.push({
        severity: "error",
        code: "review_report_incomplete",
        message: `Task ${task} Review Report section ### ${heading} is empty.`,
      })
    }
    cursor = index + 1
  }

  return {
    reviewReport: reportLines.join("\n").trimEnd(),
    diagnostics,
  }
}

function readTaskTitle(text: string) {
  return (
    text
      .split(/\r?\n/)
      .map((line) => line.match(/^#\s+(.+?)\s*#*\s*$/)?.[1]?.trim())
      .find((title) => title && title.length > 0) ?? null
  )
}

function parseHeading(line: string) {
  const match = line.match(/^(#{1,6})\s+(.+?)\s*#*\s*$/)
  if (!match) {
    return null
  }
  return {
    level: match[1].length,
    text: match[2].trim().replace(/\s+/g, " "),
  }
}

function findNextHeadingAtOrAbove(lines: string[], start: number, level: number) {
  return lines.findIndex((line, index) => {
    if (index < start) {
      return false
    }
    const heading = parseHeading(line)
    return Boolean(heading && heading.level <= level)
  })
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}

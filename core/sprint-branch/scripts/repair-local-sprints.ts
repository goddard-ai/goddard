#!/usr/bin/env bun
import * as fs from "node:fs/promises"
import path from "node:path"

import { GitCommandError, runGit } from "../src/git/command"
import {
  ensureGitInfoExcludeEntry,
  resolveGitPath,
  resolveRepositoryRoot,
} from "../src/git/repository"
import { parseReviewReport, reviewReportHeadings } from "../src/review-report"

const obsoleteSprintFilePattern = /^(?:\d+-)?(?:index|handoff)\.md$/i

/** Options for repairing local sprint task artifacts in one Git repository. */
export type LocalSprintRepairInput = {
  cwd?: string
  dryRun?: boolean
}

/** One task-file follow-up that requires task-specific agent judgment. */
export type LocalSprintManualAction = {
  relativePath: string
  sections: string[]
  message: string
}

/** Structured report emitted by the local sprint repair script. */
export type LocalSprintRepairReport = {
  ok: boolean
  dryRun: boolean
  rootDir: string
  fixes: {
    gitExcludeAdded: boolean
    removedFromGitIndex: string[]
    removedObsoleteFiles: string[]
    updatedTaskFiles: string[]
  }
  manualActions: LocalSprintManualAction[]
  notes: string[]
}

type CliArgs = {
  cwd: string
  dryRun: boolean
  json: boolean
  help: boolean
}

type TaskRepair = {
  text: string
  manualSections: string[]
}

type ReviewReportSections = {
  intro: string[]
  requiredBodies: Map<string, string[]>
  extras: string[][]
}

/** Repairs sprint task files and local Git tracking for the current repository. */
export async function repairLocalSprints(input: LocalSprintRepairInput = {}) {
  const dryRun = input.dryRun ?? false
  const rootDir = await resolveRepositoryRoot(input.cwd ?? process.cwd())
  const fixes = {
    gitExcludeAdded: await ensureSprintsExcluded(rootDir, dryRun),
    removedFromGitIndex: await untrackSprints(rootDir, dryRun),
    removedObsoleteFiles: [] as string[],
    updatedTaskFiles: [] as string[],
  }
  const manualActions: LocalSprintManualAction[] = []
  const notes: string[] = []
  const sprintsDir = path.join(rootDir, "sprints")

  if (!(await pathExists(sprintsDir))) {
    notes.push("No local sprints/ directory exists.")
    return {
      ok: true,
      dryRun,
      rootDir,
      fixes,
      manualActions,
      notes,
    } satisfies LocalSprintRepairReport
  }

  const sprintDirs = await listSprintDirs(sprintsDir)
  if (sprintDirs.length === 0) {
    notes.push("No sprint folders exist under sprints/.")
  }

  for (const sprintDir of sprintDirs) {
    const entries = await fs.readdir(sprintDir, { withFileTypes: true })
    const taskFiles: string[] = []

    for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
      if (!entry.isFile() || !entry.name.endsWith(".md")) {
        continue
      }

      const filePath = path.join(sprintDir, entry.name)
      const relativePath = path.relative(rootDir, filePath)
      if (obsoleteSprintFilePattern.test(entry.name)) {
        fixes.removedObsoleteFiles.push(relativePath)
        if (!dryRun) {
          await fs.rm(filePath, { force: true })
        }
        continue
      }

      taskFiles.push(filePath)
    }

    for (const taskPath of taskFiles) {
      const relativePath = path.relative(rootDir, taskPath)
      const text = await fs.readFile(taskPath, "utf-8")
      const repair = repairTaskReviewReport(text)

      if (repair.text !== text) {
        fixes.updatedTaskFiles.push(relativePath)
        if (!dryRun) {
          await fs.writeFile(taskPath, repair.text)
        }
      }

      if (repair.manualSections.length > 0) {
        manualActions.push({
          relativePath,
          sections: repair.manualSections,
          message: "Fill these Review Report sections with task-specific review content.",
        })
      }
    }
  }

  return {
    ok: manualActions.length === 0,
    dryRun,
    rootDir,
    fixes,
    manualActions,
    notes,
  } satisfies LocalSprintRepairReport
}

/** Formats the repair result for agents reading terminal output. */
export function formatLocalSprintRepairReport(report: LocalSprintRepairReport) {
  const action = report.dryRun ? "planned" : "complete"
  const lines = [`Local sprint repair ${action}.`]

  lines.push("")
  lines.push("Automatic fixes:")
  pushCountedList(
    lines,
    "sprints/ entries removed from Git index",
    report.fixes.removedFromGitIndex,
  )
  lines.push(
    `- ${report.fixes.gitExcludeAdded ? "Added" : "Confirmed"} .git/info/exclude entry: sprints/`,
  )
  pushCountedList(lines, "obsolete index/handoff files removed", report.fixes.removedObsoleteFiles)
  pushCountedList(lines, "task files structurally updated", report.fixes.updatedTaskFiles)

  if (report.notes.length > 0) {
    lines.push("")
    lines.push("Notes:")
    for (const note of report.notes) {
      lines.push(`- ${note}`)
    }
  }

  lines.push("")
  if (report.manualActions.length === 0) {
    lines.push("Manual follow-up: none.")
  } else {
    lines.push("Manual follow-up required before finishing sprint tasks:")
    for (const action of report.manualActions) {
      lines.push(`- ${action.relativePath}: ${action.message}`)
      lines.push(`  Sections: ${action.sections.join(", ")}`)
    }
  }

  return lines.join("\n")
}

/** Ensures local Git ignores sprints/ without staging a repository .gitignore change. */
async function ensureSprintsExcluded(rootDir: string, dryRun: boolean) {
  const excludePath = await resolveGitPath(rootDir, "info/exclude")
  let existing = ""
  if (await pathExists(excludePath)) {
    existing = await fs.readFile(excludePath, "utf-8")
  }

  if (existing.split(/\r?\n/).some((line) => line.trim() === "sprints/")) {
    return false
  }

  if (!dryRun) {
    await ensureGitInfoExcludeEntry(rootDir, "sprints/")
  }
  return true
}

/** Removes sprints/ from the Git index while preserving local task files. */
async function untrackSprints(rootDir: string, dryRun: boolean) {
  const trackedPaths = (await runGit(rootDir, ["ls-files", "-z", "--", "sprints"]))
    .split("\0")
    .filter(Boolean)

  if (trackedPaths.length > 0 && !dryRun) {
    await runGit(rootDir, ["rm", "-r", "--cached", "--quiet", "--", "sprints"])
  }

  return trackedPaths
}

/** Lists one-level sprint folders in deterministic order. */
async function listSprintDirs(sprintsDir: string) {
  const entries = await fs.readdir(sprintsDir, { withFileTypes: true })
  return entries
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(sprintsDir, entry.name))
    .sort((left, right) => path.basename(left).localeCompare(path.basename(right)))
}

/** Adds or normalizes Review Report headings without inventing task-specific content. */
function repairTaskReviewReport(text: string): TaskRepair {
  if (parseReviewReport(text, "task").diagnostics.length === 0) {
    return { text, manualSections: [] }
  }

  const newline = text.includes("\r\n") ? "\r\n" : "\n"
  const lines = text.split(/\r?\n/)
  const reportStart = lines.findIndex((line) => {
    const heading = parseHeading(line)
    return heading?.level === 2 && heading.text === "Review Report"
  })

  if (reportStart === -1) {
    return {
      text: appendReviewReport(text, newline),
      manualSections: [...reviewReportHeadings],
    }
  }

  const reportEnd = findNextHeadingAtOrAbove(lines, reportStart + 1, 2)
  const end = reportEnd === -1 ? lines.length : reportEnd
  const reportLines = lines.slice(reportStart, end)
  const sections = collectReviewReportSections(reportLines)
  const renderedReport = renderReviewReport(sections)
  const nextText = [...lines.slice(0, reportStart), ...renderedReport, ...lines.slice(end)].join(
    newline,
  )

  return {
    text: nextText,
    manualSections: missingManualSections(sections),
  }
}

/** Appends a blank Review Report scaffold to a task file. */
function appendReviewReport(text: string, newline: string) {
  const prefix = text.trimEnd()
  const separator = prefix.length === 0 ? "" : `${newline}${newline}`
  return `${prefix}${separator}${renderReviewReport(emptyReviewReportSections()).join(newline)}${newline}`
}

/** Extracts existing required and non-standard Review Report sections. */
function collectReviewReportSections(reportLines: string[]) {
  const requiredBodies = new Map<string, string[]>()
  const extras: string[][] = []
  const intro: string[] = []
  let cursor = 1

  while (cursor < reportLines.length) {
    const heading = parseHeading(reportLines[cursor])
    if (heading?.level !== 3) {
      intro.push(reportLines[cursor])
      cursor += 1
      continue
    }

    const sectionEnd = findNextHeadingAtOrAbove(reportLines, cursor + 1, 3)
    const end = sectionEnd === -1 ? reportLines.length : sectionEnd
    const body = reportLines.slice(cursor + 1, end)

    if (reviewReportHeadings.includes(heading.text) && !requiredBodies.has(heading.text)) {
      requiredBodies.set(heading.text, body)
    } else {
      extras.push(reportLines.slice(cursor, end))
    }
    cursor = end
  }

  return { intro, requiredBodies, extras }
}

/** Creates an empty section map for a newly appended report scaffold. */
function emptyReviewReportSections() {
  return {
    intro: [] as string[],
    requiredBodies: new Map<string, string[]>(),
    extras: [] as string[][],
  } satisfies ReviewReportSections
}

/** Renders Review Report sections in the canonical order expected by sprint-branch. */
function renderReviewReport(sections: ReviewReportSections) {
  const lines = ["## Review Report"]
  const intro = trimBlankEdges(sections.intro)
  if (intro.length > 0) {
    lines.push("", ...intro)
  }

  for (const heading of reviewReportHeadings) {
    lines.push("", `### ${heading}`)
    const body = trimBlankEdges(sections.requiredBodies.get(heading) ?? [])
    if (body.length > 0) {
      lines.push("", ...body)
    }
  }

  for (const extra of sections.extras) {
    const trimmed = trimBlankEdges(extra)
    if (trimmed.length > 0) {
      lines.push("", ...trimmed)
    }
  }

  return lines
}

/** Finds required sections that still need task-specific content after repair. */
function missingManualSections(sections: ReviewReportSections) {
  return reviewReportHeadings.filter((heading) => {
    const body = trimBlankEdges(sections.requiredBodies.get(heading) ?? [])
    return body.length === 0
  })
}

/** Adds one compact count line and optional itemized paths to the text report. */
function pushCountedList(lines: string[], label: string, values: string[]) {
  lines.push(`- ${label}: ${values.length}`)
  for (const value of values) {
    lines.push(`  - ${value}`)
  }
}

/** Parses minimal ATX Markdown headings used by sprint task files. */
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

/** Locates the next heading that closes the current Markdown section. */
function findNextHeadingAtOrAbove(lines: string[], start: number, level: number) {
  return lines.findIndex((line, index) => {
    if (index < start) {
      return false
    }
    const heading = parseHeading(line)
    return Boolean(heading && heading.level <= level)
  })
}

/** Removes only blank padding around a Markdown block. */
function trimBlankEdges(lines: string[]) {
  let start = 0
  let end = lines.length

  while (start < end && lines[start].trim().length === 0) {
    start += 1
  }
  while (end > start && lines[end - 1].trim().length === 0) {
    end -= 1
  }

  return lines.slice(start, end)
}

/** Checks file existence while preserving unexpected filesystem errors. */
async function pathExists(targetPath: string) {
  try {
    await fs.access(targetPath)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

/** Identifies ordinary missing-file errors from Node filesystem APIs. */
function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}

/** Parses the intentionally small CLI surface for the repair helper. */
function parseCliArgs(argv: string[]): CliArgs {
  const args = {
    cwd: process.cwd(),
    dryRun: false,
    json: false,
    help: false,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === "--dry-run") {
      args.dryRun = true
    } else if (arg === "--json") {
      args.json = true
    } else if (arg === "--help" || arg === "-h") {
      args.help = true
    } else if (arg === "--root") {
      const root = argv[index + 1]
      if (!root) {
        throw new Error("--root requires a path")
      }
      args.cwd = root
      index += 1
    } else {
      throw new Error(`Unknown argument: ${arg}`)
    }
  }

  return args
}

/** Prints usage for humans and agents invoking the repair helper directly. */
function usage() {
  return [
    "Usage: bun run ./core/sprint-branch/scripts/repair-local-sprints.ts [--dry-run] [--json] [--root <path>]",
    "",
    "Fixes local sprint artifacts that can be repaired automatically, then reports task-specific Review Report content still needing manual work.",
  ].join("\n")
}

/** Runs the repair helper as a standalone script. */
async function main() {
  const args = parseCliArgs(process.argv.slice(2))
  if (args.help) {
    console.log(usage())
    return
  }

  const report = await repairLocalSprints({ cwd: args.cwd, dryRun: args.dryRun })
  if (args.json) {
    console.log(JSON.stringify(report, null, 2))
  } else {
    console.log(formatLocalSprintRepairReport(report))
  }

  if (!report.ok) {
    process.exitCode = 1
  }
}

if (import.meta.main) {
  await main().catch((error) => {
    if (error instanceof GitCommandError) {
      console.error(`git ${error.args.join(" ")} failed`)
      console.error(error.stderr || error.message)
    } else if (error instanceof Error) {
      console.error(error.message)
    } else {
      console.error(String(error))
    }
    process.exit(1)
  })
}

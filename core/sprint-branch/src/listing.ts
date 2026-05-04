import path from "node:path"

import { resolveRepositoryRoot } from "./git/repository"
import {
  latestActedSprint,
  pushMissingLastSprintDiagnostic,
  sortSprintActivity,
} from "./state/activity"
import { findSprintStateFiles, readSprintStateFile } from "./state/io"
import type { SprintDiagnostic, SprintVisibility } from "./types"

/** One sprint state entry shown by the sprint list command. */
export type SprintListEntry = {
  sprint: string
  statePath: string
  reviewBranch: string
  visibility: SprintVisibility
  lastActedAt: string | null
}

/** Report returned by the read-only sprint list command. */
export type SprintListReport = {
  ok: boolean
  rootDir: string
  includeParked: boolean
  lastOnly: boolean
  sprints: SprintListEntry[]
  diagnostics: SprintDiagnostic[]
}

/** Lists known sprint branch state files, hiding parked sprints by default. */
export async function buildSprintList(input: {
  cwd: string
  includeParked: boolean
  lastOnly?: boolean
}) {
  const rootDir = await resolveRepositoryRoot(input.cwd)
  const diagnostics: SprintDiagnostic[] = []
  const sprints: SprintListEntry[] = []

  for (const statePath of await findSprintStateFiles(rootDir)) {
    try {
      const parsed = await readSprintStateFile(statePath, {
        defaultSprintWorktreeRoot: rootDir,
      })
      diagnostics.push(...parsed.diagnostics)
      if (
        parsed.state &&
        (input.includeParked || input.lastOnly || parsed.state.visibility === "active")
      ) {
        sprints.push({
          sprint: parsed.state.sprint,
          statePath: statePathForDisplay(rootDir, statePath),
          reviewBranch: parsed.state.branches.review,
          visibility: parsed.state.visibility,
          lastActedAt: parsed.state.lastActedAt,
        })
      }
    } catch {
      diagnostics.push({
        severity: "warning",
        code: "unreadable_sprint_state",
        message: `Could not read sprint state ${statePathForDisplay(rootDir, statePath)}.`,
      })
    }
  }

  const sortedSprints = sortSprintActivity(sprints)
  const lastSprint = latestActedSprint(sortedSprints)
  if (input.lastOnly && !lastSprint) {
    pushMissingLastSprintDiagnostic(diagnostics)
  }
  const listedSprints = input.lastOnly ? (lastSprint ? [lastSprint] : []) : sortedSprints

  return {
    ok: !diagnostics.some((diagnostic) => diagnostic.severity === "error"),
    rootDir,
    includeParked: input.includeParked,
    lastOnly: Boolean(input.lastOnly),
    sprints: listedSprints,
    diagnostics,
  } satisfies SprintListReport
}

/** Formats known sprint states for terminal output. */
export function formatSprintList(report: SprintListReport) {
  const lines = [`Sprints: ${report.lastOnly ? "last" : report.includeParked ? "all" : "active"}`]

  if (report.sprints.length === 0) {
    lines.push("  none")
  } else {
    for (const sprint of report.sprints) {
      const activity = sprint.lastActedAt ? `, last acted ${sprint.lastActedAt}` : ""
      lines.push(`  - ${sprint.sprint} [${sprint.visibility}${activity}]: ${sprint.reviewBranch}`)
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

function statePathForDisplay(rootDir: string, statePath: string) {
  const parts = statePath.split(path.sep)
  const rootIndex = parts.lastIndexOf("sprint-branch")
  if (rootIndex !== -1) {
    return path.join(".git", ...parts.slice(rootIndex))
  }
  return path.relative(rootDir, statePath)
}

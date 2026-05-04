import * as fs from "node:fs/promises"
import path from "node:path"

import type { SprintBranchState, SprintDiagnostic } from "../types"
import { findSprintStateFiles, readSprintStateFile } from "./io"
import { sprintStatePath } from "./paths"

/** One readable sprint state file with activity metadata for selection. */
export type SprintActivityCandidate = {
  sprint: string
  statePath: string
  stateRelativePath: string
  lastActedAt: string | null
  state: SprintBranchState
}

/** Reads sprint states and sorts them by most recent sprint-branch activity. */
export async function readSprintActivityCandidates(
  rootDir: string,
  options: { includeParked?: boolean } = {},
) {
  const candidates: SprintActivityCandidate[] = []

  for (const statePath of await findSprintStateFiles(rootDir)) {
    try {
      const parsed = await readSprintStateFile(statePath, {
        defaultSprintWorktreeRoot: rootDir,
      })
      if (parsed.state && (options.includeParked || parsed.state.visibility === "active")) {
        candidates.push({
          sprint: parsed.state.sprint,
          statePath,
          stateRelativePath: statePathForDisplay(rootDir, statePath),
          lastActedAt: parsed.state.lastActedAt,
          state: parsed.state,
        })
      }
    } catch {
      continue
    }
  }

  return sortSprintActivity(candidates)
}

/** Orders candidates by last activity, falling back to name order for untouched sprints. */
export function sortSprintActivity<T extends { sprint: string; lastActedAt: string | null }>(
  candidates: T[],
) {
  return [...candidates].sort(compareSprintActivity)
}

/** Returns the latest sprint with recorded activity, if any exists. */
export function latestActedSprint<T extends { lastActedAt: string | null }>(candidates: T[]) {
  return candidates.find((candidate) => candidate.lastActedAt !== null) ?? null
}

/** Adds the standard diagnostic for `-l` when no previous sprint use is recorded. */
export function pushMissingLastSprintDiagnostic(diagnostics: SprintDiagnostic[]) {
  diagnostics.push({
    severity: "error",
    code: "last_sprint_missing",
    message: "No sprint has been acted upon by sprint-branch yet.",
    suggestion: "Run a sprint command with --sprint <name> before using -l.",
  })
}

/** Updates only the private last-activity timestamp for an already selected sprint. */
export async function writeSprintLastActedAt(
  rootDir: string,
  state: SprintBranchState,
  actedAt = new Date().toISOString(),
) {
  state.lastActedAt = actedAt
  await writeSprintStateAtomic(
    await sprintStatePath(rootDir, state.sprint),
    storedSprintState(state),
  )
  return actedAt
}

/** Minimal persisted state shape rewritten when only activity metadata changes. */
type StoredSprintState = {
  sprint: string
  baseBranch: string
  sprintWorktreeRoot: string
  visibility: SprintBranchState["visibility"]
  lastActedAt: SprintBranchState["lastActedAt"]
  tasks: SprintBranchState["tasks"]
  activeStashes: SprintBranchState["activeStashes"]
  conflict: SprintBranchState["conflict"]
}

function compareSprintActivity(
  left: { sprint: string; lastActedAt: string | null },
  right: { sprint: string; lastActedAt: string | null },
) {
  const timeDelta = timestampValue(right.lastActedAt) - timestampValue(left.lastActedAt)
  return timeDelta === 0 ? left.sprint.localeCompare(right.sprint) : timeDelta
}

function timestampValue(value: string | null) {
  if (!value) {
    return 0
  }
  const timestamp = Date.parse(value)
  return Number.isNaN(timestamp) ? 0 : timestamp
}

function storedSprintState(state: SprintBranchState) {
  return {
    sprint: state.sprint,
    baseBranch: state.baseBranch,
    sprintWorktreeRoot: state.sprintWorktreeRoot,
    visibility: state.visibility,
    lastActedAt: state.lastActedAt,
    tasks: state.tasks,
    activeStashes: state.activeStashes,
    conflict: state.conflict,
  } satisfies StoredSprintState
}

async function writeSprintStateAtomic(statePath: string, state: StoredSprintState) {
  await fs.mkdir(path.dirname(statePath), { recursive: true })
  const tempPath = `${statePath}.${process.pid}.${Date.now()}.tmp`
  await fs.writeFile(tempPath, `${JSON.stringify(state, null, 2)}\n`)
  await fs.rename(tempPath, statePath)
}

function statePathForDisplay(rootDir: string, statePath: string) {
  const parts = statePath.split(path.sep)
  const rootIndex = parts.lastIndexOf("sprint-branch")
  if (rootIndex !== -1) {
    return path.join(".git", ...parts.slice(rootIndex))
  }
  return path.relative(rootDir, statePath)
}

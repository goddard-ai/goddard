import * as fs from "node:fs/promises"
import path from "node:path"

import { resolveGitCommonPath } from "../git/repository"
import { sprintStateFileName, sprintStateRoot } from "./paths"
import { parseSprintState } from "./schema"

/** Reads and validates one sprint branch state file. */
export async function readSprintStateFile(statePath: string) {
  const text = await fs.readFile(statePath, "utf-8")
  const parsed = parseSprintState(JSON.parse(text) as unknown)
  const pathSprint = path.basename(path.dirname(statePath))

  if (parsed.state && parsed.state.sprint !== pathSprint) {
    parsed.diagnostics.push({
      severity: "error",
      code: "state_sprint_mismatch",
      message:
        `${statePathForDisplay(statePath)} records sprint ${parsed.state.sprint}, ` +
        `but its state directory is ${pathSprint}.`,
    })
  }

  return parsed
}

/** Finds sprint branch state files in Git metadata. */
export async function findSprintStateFiles(rootDir: string) {
  const stateRoot = await resolveGitCommonPath(rootDir, sprintStateRoot)
  try {
    const entries = await fs.readdir(stateRoot, { withFileTypes: true })
    const statePaths = entries
      .filter((entry) => entry.isDirectory())
      .map((entry) => path.join(stateRoot, entry.name, sprintStateFileName))

    const existing = await Promise.all(
      statePaths.map(async (statePath) => ({
        statePath,
        exists: await asyncBooleanPathExists(statePath),
      })),
    )
    return existing.filter((entry) => entry.exists).map((entry) => entry.statePath)
  } catch (error) {
    if (isMissingFileError(error)) {
      return []
    }
    throw error
  }
}

async function asyncBooleanPathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}

function statePathForDisplay(statePath: string) {
  const parts = statePath.split(path.sep)
  const rootIndex = parts.lastIndexOf("sprint-branch")
  if (rootIndex !== -1) {
    return path.join(".git", ...parts.slice(rootIndex))
  }
  return statePath
}

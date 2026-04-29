import * as fs from "node:fs/promises"
import path from "node:path"

import { resolveGitPath } from "./git/repository"
import type { SprintConflictState } from "./types"

/** Reads worktree-local conflict metadata while Git owns the worktree. */
export async function readTransientConflict(rootDir: string, sprint: string) {
  try {
    return JSON.parse(
      await fs.readFile(await transientConflictPath(rootDir, sprint), "utf-8"),
    ) as SprintConflictState
  } catch (error) {
    if (isMissingFileError(error) || error instanceof SyntaxError) {
      return null
    }
    throw error
  }
}

/** Writes conflict metadata under .git so canonical state is not advanced mid-rebase. */
export async function writeTransientConflict(
  rootDir: string,
  sprint: string,
  conflict: SprintConflictState | null,
) {
  if (!conflict) {
    return
  }

  const markerPath = await transientConflictPath(rootDir, sprint)
  await fs.mkdir(path.dirname(markerPath), { recursive: true })
  await fs.writeFile(markerPath, `${JSON.stringify(conflict, null, 2)}\n`)
}

/** Clears transient conflict metadata after the owning sprint transition completes. */
export async function clearTransientConflict(rootDir: string, sprint: string) {
  await fs.rm(await transientConflictPath(rootDir, sprint), { force: true })
}

async function transientConflictPath(rootDir: string, sprint: string) {
  return resolveGitPath(rootDir, `sprint-branch/${sprint}.conflict.json`)
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}

import * as fs from "node:fs/promises"
import path from "node:path"

import { resolveGitCommonPath } from "../git/repository"
import type { SprintBranchState, SprintContext, SprintMutationReport } from "../types"

/** Runs one mutating command under a Git-private sprint lock file. */
export async function withSprintLock(
  context: SprintContext,
  state: SprintBranchState,
  commandName: string,
  run: () => Promise<SprintMutationReport>,
) {
  const lockDisplayPath = path.join(".git", "sprint-branch", `${context.sprint}.lock`)
  const lockPath = await resolveGitCommonPath(
    context.rootDir,
    `sprint-branch/${context.sprint}.lock`,
  )
  const lock = {
    command: commandName,
    createdAt: new Date().toISOString(),
    pid: process.pid,
  }
  let handle: fs.FileHandle | null = null

  try {
    await fs.mkdir(path.dirname(lockPath), { recursive: true })
    handle = await fs.open(lockPath, "wx")
    await handle.writeFile(`${JSON.stringify(lock, null, 2)}\n`)
    await handle.close()
    handle = null
    return await run()
  } catch (error) {
    if (isAlreadyExistsError(error)) {
      return {
        ok: false,
        command: commandName,
        dryRun: false,
        executed: false,
        sprint: state.sprint,
        currentBranch: context.currentBranch,
        summary: `Sprint ${state.sprint} is locked by another branch operation.`,
        requiresCleanWorkingTree: true,
        gitOperations: [],
        stateFiles: [lockDisplayPath],
        conflictHandling:
          "Remove the lock only after confirming no sprint-branch command is running.",
        diagnostics: [
          {
            severity: "error",
            code: "lock_exists",
            message: `Lock file ${lockDisplayPath} already exists.`,
          },
        ],
        state,
      } satisfies SprintMutationReport
    }
    throw error
  } finally {
    if (handle) {
      await handle.close()
    }
    await fs.rm(lockPath, { force: true })
  }
}

function isAlreadyExistsError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "EEXIST"
  )
}

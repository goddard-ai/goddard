import * as fs from "node:fs/promises"
import { hostname } from "node:os"
import path from "node:path"
import { setTimeout as sleep } from "node:timers/promises"

import { resolveGitCommonPath } from "../git/repository"
import type { SprintBranchState, SprintContext, SprintMutationReport } from "../types"

const sprintLockPollMs = 100
const sprintLockStaleAfterMs = 10 * 60 * 1000

/** Metadata persisted by the sprint branch operation lock file. */
type SprintLockFile = {
  command?: string
  createdAt?: string
  pid?: number
  hostname?: string
}

/** A sprint lock held by a process that still appears active. */
export type ActiveSprintLock = SprintLockFile & {
  status: "active"
  path: string
  displayPath: string
}

/** A sprint lock that can be removed before continuing. */
type StaleSprintLock = SprintLockFile & {
  status: "stale"
  path: string
  displayPath: string
  staleReason: string
}

/** The current state of the sprint branch operation lock. */
export type SprintLockInspection =
  | { status: "absent"; path: string; displayPath: string }
  | ActiveSprintLock
  | StaleSprintLock

/** Runs one mutating command under a Git-private sprint lock file. */
export async function withSprintLock(
  context: SprintContext,
  state: SprintBranchState,
  commandName: string,
  run: () => Promise<SprintMutationReport>,
) {
  const lockDisplayPath = sprintLockDisplayPath(context.sprint)
  const lockPath = await sprintLockPath(context)
  const lock = {
    command: commandName,
    createdAt: new Date().toISOString(),
    pid: process.pid,
    hostname: hostname(),
  }
  let handle: fs.FileHandle | null = null
  let acquired = false

  try {
    await fs.mkdir(path.dirname(lockPath), { recursive: true })
    handle = await openSprintLockFile(context, lockPath)
    acquired = true
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
    if (acquired) {
      await fs.rm(lockPath, { force: true })
    }
  }
}

/** Returns the display path used in user-facing sprint lock diagnostics. */
export function sprintLockDisplayPath(sprint: string) {
  return path.join(".git", "sprint-branch", `${sprint}.lock`)
}

/** Returns the Git-private sprint lock path shared by linked worktrees. */
export async function sprintLockPath(context: SprintContext) {
  return resolveGitCommonPath(context.rootDir, `sprint-branch/${context.sprint}.lock`)
}

/** Reads the sprint lock and classifies whether it is live, stale, or absent. */
export async function inspectSprintLock(context: SprintContext) {
  const lockPath = await sprintLockPath(context)
  const displayPath = sprintLockDisplayPath(context.sprint)
  let lock: SprintLockFile | null = null

  try {
    lock = parseSprintLockFile(await fs.readFile(lockPath, "utf-8"))
  } catch (error) {
    if (isErrno(error, "ENOENT")) {
      return { status: "absent", path: lockPath, displayPath } satisfies SprintLockInspection
    }
    throw error
  }

  if (!lock) {
    return {
      status: "stale",
      path: lockPath,
      displayPath,
      staleReason: "invalid lock contents",
    } satisfies SprintLockInspection
  }

  const staleReason = sprintLockStaleReason(lock)
  if (staleReason) {
    return {
      ...lock,
      status: "stale",
      path: lockPath,
      displayPath,
      staleReason,
    } satisfies SprintLockInspection
  }

  return {
    ...lock,
    status: "active",
    path: lockPath,
    displayPath,
  } satisfies SprintLockInspection
}

/** Removes stale sprint locks and returns the lock state observed before removal. */
export async function reclaimStaleSprintLock(context: SprintContext) {
  const inspection = await inspectSprintLock(context)
  if (inspection.status === "stale") {
    await fs.rm(inspection.path, { force: true })
  }
  return inspection
}

/** Waits for an active sprint branch operation to release its lock. */
export async function waitForSprintLockRelease(input: {
  context: SprintContext
  signal?: AbortSignal
  onWait?: (lock: ActiveSprintLock) => void | Promise<void>
}) {
  let waitReported = false

  while (input.signal?.aborted !== true) {
    const inspection = await reclaimStaleSprintLock(input.context)
    if (inspection.status !== "active") {
      return true
    }

    if (!waitReported) {
      waitReported = true
      await input.onWait?.(inspection)
    }

    try {
      await sleep(sprintLockPollMs, undefined, { signal: input.signal })
    } catch (error) {
      if (isAbortError(error)) {
        return false
      }
      throw error
    }
  }

  return false
}

async function openSprintLockFile(context: SprintContext, lockPath: string) {
  try {
    return await fs.open(lockPath, "wx")
  } catch (error) {
    if (!isAlreadyExistsError(error)) {
      throw error
    }

    const inspection = await reclaimStaleSprintLock(context)
    if (inspection.status === "stale") {
      return await fs.open(lockPath, "wx")
    }

    throw error
  }
}

function parseSprintLockFile(text: string) {
  try {
    const value = JSON.parse(text) as SprintLockFile
    return value && typeof value === "object" ? value : null
  } catch {
    return null
  }
}

function sprintLockStaleReason(lock: SprintLockFile) {
  if (typeof lock.createdAt === "string") {
    const createdAt = Date.parse(lock.createdAt)
    if (!Number.isNaN(createdAt) && Date.now() - createdAt > sprintLockStaleAfterMs) {
      return "lock is older than the stale timeout"
    }
  }

  if (lock.hostname && lock.hostname !== hostname()) {
    return null
  }

  if (typeof lock.pid === "number") {
    return isProcessRunning(lock.pid) ? null : `process ${lock.pid} is no longer running`
  }

  return "lock owner pid is missing"
}

function isProcessRunning(pid: number) {
  if (!Number.isInteger(pid) || pid <= 0) {
    return false
  }

  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return !isErrno(error, "ESRCH")
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

function isAbortError(error: unknown) {
  return error instanceof Error && error.name === "AbortError"
}

function isErrno(error: unknown, code: string) {
  return error instanceof Error && "code" in error && error.code === code
}

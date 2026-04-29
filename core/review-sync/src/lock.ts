/** Durable lock handling for mutating review-sync operations. */
import { mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { hostname } from "node:os"
import { join, resolve } from "node:path"

import { UserError } from "./errors.ts"
import { isNodeErrorWithCode, isProcessAlive } from "./git.ts"
import { resolveSessionDir } from "./state.ts"
import { lockStaleAfterMs, type SessionState } from "./types.ts"

/** Acquires the session lock for one mutating operation and releases it on completion. */
export async function withSessionLock<T>(session: SessionState, work: () => Promise<T>) {
  const lockDir = join(resolveSessionDir(session.repoCommonDir, session.sessionId), "lock")
  const release = await acquireLock(lockDir)
  try {
    return await work()
  } finally {
    await release()
  }
}

/** Creates an atomic lock directory, reclaiming stale local-process locks. */
async function acquireLock(lockDir: string) {
  await mkdir(resolve(lockDir, ".."), { recursive: true })
  try {
    await mkdir(lockDir)
  } catch (error) {
    if (!isNodeErrorWithCode(error, "EEXIST")) {
      throw error
    }
    if (!(await isLockStale(lockDir))) {
      throw new UserError(`Review sync session is already locked: ${lockDir}`)
    }
    await rm(lockDir, { recursive: true, force: true })
    await mkdir(lockDir)
  }

  await writeFile(
    join(lockDir, "owner.json"),
    JSON.stringify({
      pid: process.pid,
      hostname: hostname(),
      acquiredAt: Date.now(),
    }),
  )

  return async () => {
    await rm(lockDir, { recursive: true, force: true }).catch(() => {})
  }
}

/** Treats locks from dead same-host processes, malformed locks, or old locks as stale. */
async function isLockStale(lockDir: string) {
  try {
    const owner = JSON.parse(await readFile(join(lockDir, "owner.json"), "utf-8")) as {
      pid?: number
      hostname?: string
      acquiredAt?: number
    }
    if (typeof owner.acquiredAt === "number" && Date.now() - owner.acquiredAt > lockStaleAfterMs) {
      return true
    }
    if (owner.hostname && owner.hostname !== hostname()) {
      return false
    }
    if (typeof owner.pid === "number") {
      return !isProcessAlive(owner.pid)
    }
    return true
  } catch {
    return true
  }
}

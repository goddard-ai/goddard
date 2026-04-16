/** Git-backed sync-session host for one primary checkout and one linked session worktree. */
import type { DaemonSessionId } from "@goddard-ai/schema/common/params"
import { randomUUID } from "node:crypto"
import { mkdir, readFile, readdir, realpath, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"

import { runCommand } from "./process.ts"

/** Conflict preference used when one sync cycle merges both sides. */
export type WorktreeSyncConflictPreference = "worktree"

/** Current mounted sync status reported to daemon clients. */
export type WorktreeSyncStatus = "mounted"

/** Live sync-session state merged into one daemon worktree response. */
export interface WorktreeSyncSessionState {
  sessionId: DaemonSessionId
  status: WorktreeSyncStatus
  conflictPreference: WorktreeSyncConflictPreference
  primaryDir: string
  worktreeDir: string
  commonDir: string
  baseOid: string
  primaryOriginalHeadOid: string
  primaryOriginalSymbolicRef: string | null
  primaryOriginalBranchTipOid: string | null
  primaryLatestSnapshotOid: string | null
  worktreeLatestSnapshotOid: string | null
  resultSnapshotOid: string | null
  primaryRecoverySnapshotOid: string | null
  lastSyncAt: number | null
}

type WorktreeSyncMetadata = {
  sessionId: DaemonSessionId
  primaryDir: string
  worktreeDir: string
  commonDir: string
  baseOid: string
  status: WorktreeSyncStatus
  conflictPreference: WorktreeSyncConflictPreference
  primaryOriginalHeadOid: string
  primaryOriginalSymbolicRef: string | null
  primaryOriginalBranchTipOid: string | null
  mountedAt: number
  lastSyncAt: number | null
}

const syncRefNames = {
  base: (sessionId: DaemonSessionId) => `refs/goddard/worktree-sync/${sessionId}/base`,
  primaryPreMount: (sessionId: DaemonSessionId) =>
    `refs/goddard/worktree-sync/${sessionId}/primary/pre_mount`,
  primaryLatest: (sessionId: DaemonSessionId) =>
    `refs/goddard/worktree-sync/${sessionId}/primary/latest`,
  worktreeLatest: (sessionId: DaemonSessionId) =>
    `refs/goddard/worktree-sync/${sessionId}/worktree/latest`,
  resultLatest: (sessionId: DaemonSessionId) =>
    `refs/goddard/worktree-sync/${sessionId}/result/latest`,
  primaryRecoveryLatest: (sessionId: DaemonSessionId) =>
    `refs/goddard/worktree-sync/${sessionId}/primary/recovery/latest`,
}

/** Scans the shared Git metadata directory for one mounted sync session targeting the primary checkout. */
export async function findMountedWorktreeSyncSessionByPrimaryDir(primaryDir: string) {
  const normalizedPrimaryDir = await normalizePath(primaryDir)
  const commonDir = await resolveGitCommonDir(normalizedPrimaryDir)
  if (!commonDir) {
    return null
  }

  const metadataDir = resolveSyncMetadataDir(commonDir)
  try {
    const entries = await readdir(metadataDir)
    for (const entry of entries) {
      if (!entry.endsWith(".json")) {
        continue
      }

      const metadata = await readMetadata(join(metadataDir, entry))
      if (!metadata) {
        continue
      }

      if (metadata.primaryDir === normalizedPrimaryDir) {
        return metadata
      }
    }
  } catch {
    return null
  }

  return null
}

/** Rehydrates one daemon-owned sync session from Git metadata and shared refs on demand. */
export class WorktreeSyncSessionHost {
  readonly #sessionId
  readonly #primaryDir
  readonly #worktreeDir

  constructor(input: { sessionId: DaemonSessionId; primaryDir: string; worktreeDir: string }) {
    this.#sessionId = input.sessionId
    this.#primaryDir = input.primaryDir
    this.#worktreeDir = input.worktreeDir
  }

  /** Returns the current mounted sync state when the session is mounted. */
  async inspect() {
    const commonDir = await resolveGitCommonDir(this.#primaryDir)
    if (!commonDir) {
      return null
    }

    const metadata = await readMetadata(resolveMetadataPath(commonDir, this.#sessionId))
    if (!metadata) {
      return null
    }

    return await createStateFromMetadata(metadata)
  }

  /** Mounts one worktree sync session and materializes the initial mirrored result. */
  async mount() {
    const primaryDir = await normalizePath(this.#primaryDir)
    const worktreeDir = await normalizePath(this.#worktreeDir)
    const commonDir = await resolveRequiredCommonDir(primaryDir, worktreeDir)

    return await withRepoLock(commonDir, async () => {
      const existing = await this.inspect()
      if (existing) {
        return existing
      }

      const conflictingSession = await findMountedWorktreeSyncSessionByPrimaryDir(primaryDir)
      if (conflictingSession && conflictingSession.sessionId !== this.#sessionId) {
        throw new Error(
          `Another mounted sync session already owns ${primaryDir}: ${conflictingSession.sessionId}`,
        )
      }

      const worktreeHead = await resolveRequiredHeadOid(worktreeDir)
      const primaryHead = await resolveRequiredHeadOid(primaryDir)
      const primarySymbolicRef = await resolveSymbolicRef(primaryDir)
      const primaryBranchTipOid = primarySymbolicRef
        ? await resolveRefOid(primaryDir, primarySymbolicRef)
        : null
      const metadata: WorktreeSyncMetadata = {
        sessionId: this.#sessionId,
        primaryDir,
        worktreeDir,
        commonDir,
        baseOid: worktreeHead,
        status: "mounted",
        conflictPreference: "worktree",
        primaryOriginalHeadOid: primaryHead,
        primaryOriginalSymbolicRef: primarySymbolicRef,
        primaryOriginalBranchTipOid: primaryBranchTipOid,
        mountedAt: Date.now(),
        lastSyncAt: null,
      }

      await setRef(primaryDir, syncRefNames.base(this.#sessionId), metadata.baseOid)
      await setRef(
        primaryDir,
        syncRefNames.primaryPreMount(this.#sessionId),
        await captureSnapshot(primaryDir, `${this.#sessionId}:primary:pre-mount`),
      )
      await setRef(primaryDir, syncRefNames.primaryLatest(this.#sessionId), null)
      await setRef(primaryDir, syncRefNames.worktreeLatest(this.#sessionId), null)
      await setRef(primaryDir, syncRefNames.resultLatest(this.#sessionId), null)
      await setRef(primaryDir, syncRefNames.primaryRecoveryLatest(this.#sessionId), null)
      await writeMetadata(metadata)
      await detachAndResetCheckout(primaryDir, metadata.baseOid)
      const result = await syncOnceLocked(metadata)

      return result.state!
    })
  }

  /** Runs one immediate sync cycle and returns the updated state with any warnings. */
  async syncOnce() {
    const commonDir = await resolveGitCommonDir(this.#primaryDir)
    if (!commonDir) {
      throw new Error(`Primary checkout is no longer a git repository: ${this.#primaryDir}`)
    }

    return await withRepoLock(commonDir, async () => {
      const metadata = await readRequiredMetadata(resolveMetadataPath(commonDir, this.#sessionId))
      return await syncOnceLocked(metadata)
    })
  }

  /** Restores the primary checkout and removes all Git-owned sync state for the session. */
  async unmount() {
    const commonDir = await resolveGitCommonDir(this.#primaryDir)
    if (!commonDir) {
      return { state: null, warnings: [] }
    }

    return await withRepoLock(commonDir, async () => {
      const metadata = await readMetadata(resolveMetadataPath(commonDir, this.#sessionId))
      if (!metadata) {
        return { state: null, warnings: [] }
      }

      const warnings = await unmountLocked(metadata)
      return {
        state: null,
        warnings,
      }
    })
  }
}

async function syncOnceLocked(metadata: WorktreeSyncMetadata) {
  await verifyMountedHeads(metadata)

  const primaryLatestSnapshotOid = await captureSnapshot(
    metadata.primaryDir,
    `${metadata.sessionId}:primary:latest`,
  )
  const worktreeLatestSnapshotOid = await captureSnapshot(
    metadata.worktreeDir,
    `${metadata.sessionId}:worktree:latest`,
  )

  await setRef(
    metadata.primaryDir,
    syncRefNames.primaryLatest(metadata.sessionId),
    primaryLatestSnapshotOid,
  )
  await setRef(
    metadata.primaryDir,
    syncRefNames.worktreeLatest(metadata.sessionId),
    worktreeLatestSnapshotOid,
  )

  const computation = await computeResultSnapshot(metadata, {
    primarySnapshotOid: primaryLatestSnapshotOid,
    worktreeSnapshotOid: worktreeLatestSnapshotOid,
  })

  await setRef(
    metadata.primaryDir,
    syncRefNames.resultLatest(metadata.sessionId),
    computation.resultSnapshotOid,
  )
  await setRef(
    metadata.primaryDir,
    syncRefNames.primaryRecoveryLatest(metadata.sessionId),
    computation.primaryRecoverySnapshotOid,
  )

  await rebuildCheckoutFromResult(metadata.primaryDir, metadata.baseOid, computation)
  await rebuildCheckoutFromResult(metadata.worktreeDir, metadata.baseOid, computation)

  metadata.lastSyncAt = Date.now()
  await writeMetadata(metadata)

  return {
    state: await createStateFromMetadata(metadata),
    warnings: computation.warnings,
  }
}

async function unmountLocked(metadata: WorktreeSyncMetadata) {
  const warnings: string[] = []

  await resetCheckoutToBase(metadata.primaryDir, metadata.baseOid)
  await resetCheckoutToBase(metadata.worktreeDir, metadata.baseOid)

  const restored = await restorePrimaryHead(metadata)
  if (restored.warning) {
    warnings.push(restored.warning)
  }

  const preMountSnapshotOid = await resolveRefOid(
    metadata.primaryDir,
    syncRefNames.primaryPreMount(metadata.sessionId),
  )
  if (preMountSnapshotOid) {
    await applySnapshot(metadata.primaryDir, preMountSnapshotOid)
  }

  await deleteSessionRefs(metadata.primaryDir, metadata.sessionId)
  await rm(resolveMetadataPath(metadata.commonDir, metadata.sessionId), { force: true })
  return warnings
}

async function computeResultSnapshot(
  metadata: WorktreeSyncMetadata,
  input: {
    primarySnapshotOid: string | null
    worktreeSnapshotOid: string | null
  },
) {
  if (!input.primarySnapshotOid && !input.worktreeSnapshotOid) {
    return {
      resultSnapshotOid: null,
      primaryRecoverySnapshotOid: null,
      warnings: [],
    }
  }

  if (!input.primarySnapshotOid) {
    return {
      resultSnapshotOid: input.worktreeSnapshotOid,
      primaryRecoverySnapshotOid: null,
      warnings: [],
    }
  }

  if (!input.worktreeSnapshotOid) {
    return {
      resultSnapshotOid: input.primarySnapshotOid,
      primaryRecoverySnapshotOid: null,
      warnings: [],
    }
  }

  const scratchDir = resolveScratchDir(metadata.sessionId)
  const warnings: string[] = []

  try {
    await createScratchWorktree(metadata.primaryDir, scratchDir, metadata.baseOid)
    const worktreeCommitOid = await materializeSnapshotCommit({
      cwd: scratchDir,
      baseOid: metadata.baseOid,
      snapshotOid: input.worktreeSnapshotOid,
      label: `${metadata.sessionId}:worktree-materialized`,
    })
    const primaryCommitOid = await materializeSnapshotCommit({
      cwd: scratchDir,
      baseOid: metadata.baseOid,
      snapshotOid: input.primarySnapshotOid,
      label: `${metadata.sessionId}:primary-materialized`,
    })

    await resetCheckoutToBase(scratchDir, worktreeCommitOid)
    const mergeResult = await runGit(
      scratchDir,
      ["merge", "-X", "ours", "--no-commit", primaryCommitOid],
      { allowFailure: true },
    )
    const conflictedPaths = await resolveConflictedPaths(scratchDir)
    if (mergeResult.status !== 0 && conflictedPaths.length === 0) {
      throw new Error(mergeResult.stderr || mergeResult.stdout || "git merge failed")
    }

    for (const conflictedPath of conflictedPaths) {
      await restoreConflictToWorktreeVersion(scratchDir, worktreeCommitOid, conflictedPath)
      warnings.push(`Preferred worktree version for conflicted path ${conflictedPath}`)
    }

    await createOrdinaryCommit(scratchDir, `${metadata.sessionId}:merged-result`)
    const mergedCommitOid = await resolveRequiredHeadOid(scratchDir)
    await resetCheckoutToBase(scratchDir, metadata.baseOid)
    await materializeCommitTreeAsDirtyState(scratchDir, metadata.baseOid, mergedCommitOid)

    return {
      resultSnapshotOid: await captureSnapshot(scratchDir, `${metadata.sessionId}:result`),
      primaryRecoverySnapshotOid: input.primarySnapshotOid,
      warnings,
    }
  } finally {
    await removeScratchWorktree(metadata.primaryDir, scratchDir)
  }
}

async function rebuildCheckoutFromResult(
  cwd: string,
  baseOid: string,
  result: {
    resultSnapshotOid: string | null
  },
) {
  await resetCheckoutToBase(cwd, baseOid)
  if (result.resultSnapshotOid) {
    await applySnapshot(cwd, result.resultSnapshotOid)
  }
}

async function materializeSnapshotCommit(input: {
  cwd: string
  baseOid: string
  snapshotOid: string
  label: string
}) {
  await resetCheckoutToBase(input.cwd, input.baseOid)
  await applySnapshot(input.cwd, input.snapshotOid)
  await createOrdinaryCommit(input.cwd, input.label)
  return await resolveRequiredHeadOid(input.cwd)
}

async function materializeCommitTreeAsDirtyState(cwd: string, baseOid: string, commitOid: string) {
  const diffResult = await runGit(cwd, ["diff", "--binary", baseOid, commitOid])
  if (!diffResult.stdout.trim()) {
    return
  }

  await runGit(cwd, ["apply", "--index", "--3way", "--allow-empty"], {
    stdin: diffResult.stdout,
  })
}

async function createOrdinaryCommit(cwd: string, label: string) {
  await runGit(cwd, ["add", "-A"])
  await runGit(cwd, [
    "-c",
    "user.name=Goddard",
    "-c",
    "user.email=goddard@local",
    "commit",
    "--allow-empty",
    "-m",
    label,
  ])
}

async function createScratchWorktree(primaryDir: string, scratchDir: string, baseOid: string) {
  await runGit(primaryDir, ["worktree", "add", "--detach", scratchDir, baseOid], {
    stdin: "ignore",
  })
}

async function removeScratchWorktree(primaryDir: string, scratchDir: string) {
  await runGit(primaryDir, ["worktree", "remove", "--force", scratchDir], {
    allowFailure: true,
    stdin: "ignore",
  })
  await rm(scratchDir, { recursive: true, force: true }).catch(() => {})
}

async function resolveConflictedPaths(cwd: string) {
  const unresolved = await runGit(cwd, ["ls-files", "-u"], { allowFailure: true })
  const paths = new Set<string>()

  for (const line of unresolved.stdout.split("\n")) {
    const trimmed = line.trim()
    if (!trimmed) {
      continue
    }

    const [, pathPart] = trimmed.split("\t")
    if (pathPart) {
      paths.add(pathPart)
    }
  }

  return [...paths]
}

async function restoreConflictToWorktreeVersion(
  cwd: string,
  worktreeCommitOid: string,
  conflictedPath: string,
) {
  const oursExists = await objectExists(cwd, `${worktreeCommitOid}:${conflictedPath}`)
  if (oursExists) {
    await runGit(cwd, ["checkout", worktreeCommitOid, "--", conflictedPath])
    await runGit(cwd, ["add", "--", conflictedPath])
    return
  }

  await rm(join(cwd, conflictedPath), { recursive: true, force: true }).catch(() => {})
  await runGit(cwd, ["rm", "-f", "--ignore-unmatch", "--", conflictedPath], {
    allowFailure: true,
  })
}

async function restorePrimaryHead(metadata: WorktreeSyncMetadata) {
  if (!metadata.primaryOriginalSymbolicRef) {
    await runGit(metadata.primaryDir, ["checkout", "--detach", metadata.primaryOriginalHeadOid])
    return { warning: null }
  }

  const currentTip = await resolveRefOid(metadata.primaryDir, metadata.primaryOriginalSymbolicRef)
  if (
    currentTip &&
    metadata.primaryOriginalBranchTipOid &&
    currentTip === metadata.primaryOriginalBranchTipOid
  ) {
    await runGit(metadata.primaryDir, [
      "checkout",
      metadata.primaryOriginalSymbolicRef.replace(/^refs\/heads\//, ""),
    ])
    return { warning: null }
  }

  await runGit(metadata.primaryDir, ["checkout", "--detach", metadata.primaryOriginalHeadOid])
  return {
    warning: `Primary branch ${metadata.primaryOriginalSymbolicRef} moved while sync was mounted; restored detached HEAD at the original commit instead.`,
  }
}

async function verifyMountedHeads(metadata: WorktreeSyncMetadata) {
  const [primaryHead, worktreeHead] = await Promise.all([
    resolveRequiredHeadOid(metadata.primaryDir),
    resolveRequiredHeadOid(metadata.worktreeDir),
  ])

  if (primaryHead !== metadata.baseOid) {
    throw new Error(
      `Primary checkout HEAD ${primaryHead} no longer matches mounted base ${metadata.baseOid}.`,
    )
  }

  if (worktreeHead !== metadata.baseOid) {
    throw new Error(
      `Session worktree HEAD ${worktreeHead} no longer matches mounted base ${metadata.baseOid}.`,
    )
  }
}

async function detachAndResetCheckout(cwd: string, baseOid: string) {
  await runGit(cwd, ["checkout", "--detach", baseOid])
  await resetCheckoutToBase(cwd, baseOid)
}

async function resetCheckoutToBase(cwd: string, baseOid: string) {
  await runGit(cwd, ["reset", "--hard", baseOid])
  await runGit(cwd, ["clean", "-fd"])
}

async function applySnapshot(cwd: string, snapshotOid: string) {
  await runGit(cwd, ["stash", "apply", "--index", snapshotOid])
}

async function captureSnapshot(cwd: string, label: string) {
  const previousTop = await resolveRefOid(cwd, "refs/stash")
  await runGit(cwd, ["stash", "push", "-u", "-m", label], { allowFailure: true })
  const nextTop = await resolveRefOid(cwd, "refs/stash")
  if (!nextTop || nextTop === previousTop) {
    return null
  }

  await runGit(cwd, ["stash", "apply", "--index", nextTop])
  await runGit(cwd, ["stash", "drop", "stash@{0}"])
  return nextTop
}

async function createStateFromMetadata(metadata: WorktreeSyncMetadata) {
  return {
    sessionId: metadata.sessionId,
    status: metadata.status,
    conflictPreference: metadata.conflictPreference,
    primaryDir: metadata.primaryDir,
    worktreeDir: metadata.worktreeDir,
    commonDir: metadata.commonDir,
    baseOid: metadata.baseOid,
    primaryOriginalHeadOid: metadata.primaryOriginalHeadOid,
    primaryOriginalSymbolicRef: metadata.primaryOriginalSymbolicRef,
    primaryOriginalBranchTipOid: metadata.primaryOriginalBranchTipOid,
    primaryLatestSnapshotOid: await resolveRefOid(
      metadata.primaryDir,
      syncRefNames.primaryLatest(metadata.sessionId),
    ),
    worktreeLatestSnapshotOid: await resolveRefOid(
      metadata.primaryDir,
      syncRefNames.worktreeLatest(metadata.sessionId),
    ),
    resultSnapshotOid: await resolveRefOid(
      metadata.primaryDir,
      syncRefNames.resultLatest(metadata.sessionId),
    ),
    primaryRecoverySnapshotOid: await resolveRefOid(
      metadata.primaryDir,
      syncRefNames.primaryRecoveryLatest(metadata.sessionId),
    ),
    lastSyncAt: metadata.lastSyncAt,
  } satisfies WorktreeSyncSessionState
}

async function deleteSessionRefs(primaryDir: string, sessionId: DaemonSessionId) {
  await Promise.all([
    deleteRef(primaryDir, syncRefNames.base(sessionId)),
    deleteRef(primaryDir, syncRefNames.primaryPreMount(sessionId)),
    deleteRef(primaryDir, syncRefNames.primaryLatest(sessionId)),
    deleteRef(primaryDir, syncRefNames.worktreeLatest(sessionId)),
    deleteRef(primaryDir, syncRefNames.resultLatest(sessionId)),
    deleteRef(primaryDir, syncRefNames.primaryRecoveryLatest(sessionId)),
  ])
}

async function setRef(cwd: string, refName: string, oid: string | null) {
  if (!oid) {
    await deleteRef(cwd, refName)
    return
  }

  await runGit(cwd, ["update-ref", refName, oid])
}

async function deleteRef(cwd: string, refName: string) {
  await runGit(cwd, ["update-ref", "-d", refName], { allowFailure: true })
}

async function writeMetadata(metadata: WorktreeSyncMetadata) {
  const metadataDir = resolveSyncMetadataDir(metadata.commonDir)
  await mkdir(metadataDir, { recursive: true })
  await writeFile(
    resolveMetadataPath(metadata.commonDir, metadata.sessionId),
    JSON.stringify(metadata),
  )
}

async function readRequiredMetadata(metadataPath: string) {
  const metadata = await readMetadata(metadataPath)
  if (!metadata) {
    throw new Error(`Mounted sync metadata is missing: ${metadataPath}`)
  }

  return metadata
}

async function readMetadata(metadataPath: string) {
  try {
    const content = await readFile(metadataPath, "utf-8")
    const parsed = JSON.parse(content) as WorktreeSyncMetadata
    if (!parsed || typeof parsed.sessionId !== "string" || typeof parsed.baseOid !== "string") {
      return null
    }

    return parsed
  } catch {
    return null
  }
}

async function resolveRequiredCommonDir(primaryDir: string, worktreeDir: string) {
  const [primaryCommonDir, worktreeCommonDir] = await Promise.all([
    resolveGitCommonDir(primaryDir),
    resolveGitCommonDir(worktreeDir),
  ])
  if (!primaryCommonDir || !worktreeCommonDir || primaryCommonDir !== worktreeCommonDir) {
    throw new Error(
      `Primary checkout ${primaryDir} and worktree ${worktreeDir} must share one Git common dir.`,
    )
  }

  return primaryCommonDir
}

async function resolveRequiredHeadOid(cwd: string) {
  const headOid = await resolveHeadOid(cwd)
  if (!headOid) {
    throw new Error(`Failed to resolve HEAD for ${cwd}`)
  }

  return headOid
}

async function resolveHeadOid(cwd: string) {
  const result = await runGit(cwd, ["rev-parse", "HEAD"])
  return result.stdout.trim() || null
}

async function resolveSymbolicRef(cwd: string) {
  const result = await runGit(cwd, ["symbolic-ref", "-q", "HEAD"], { allowFailure: true })
  return result.stdout.trim() || null
}

async function resolveRefOid(cwd: string, refName: string) {
  const result = await runGit(cwd, ["rev-parse", "--verify", "-q", refName], { allowFailure: true })
  return result.stdout.trim() || null
}

async function resolveGitCommonDir(cwd: string) {
  const result = await runGit(cwd, ["rev-parse", "--git-common-dir"], { allowFailure: true })
  const value = result.stdout.trim()
  return value ? await normalizePath(resolve(cwd, value)) : null
}

async function normalizePath(value: string) {
  return await realpath(resolve(value))
}

function resolveSyncMetadataDir(commonDir: string) {
  return join(commonDir, "goddard", "worktree-sync")
}

function resolveMetadataPath(commonDir: string, sessionId: DaemonSessionId) {
  return join(resolveSyncMetadataDir(commonDir), `${sessionId}.json`)
}

function resolveLockDir(commonDir: string) {
  return join(resolveSyncMetadataDir(commonDir), "lock")
}

function resolveScratchDir(sessionId: DaemonSessionId) {
  return join(tmpdir(), `goddard-worktree-sync-${sessionId}-${randomUUID()}`)
}

async function withRepoLock<T>(commonDir: string, work: () => Promise<T>) {
  const release = await acquireRepoLock(commonDir)
  try {
    return await work()
  } finally {
    await release()
  }
}

async function acquireRepoLock(commonDir: string) {
  const metadataDir = resolveSyncMetadataDir(commonDir)
  const lockDir = resolveLockDir(commonDir)
  await mkdir(metadataDir, { recursive: true })

  for (let attempt = 0; attempt < 200; attempt += 1) {
    try {
      await mkdir(lockDir)
      await writeFile(
        join(lockDir, "owner.json"),
        JSON.stringify({
          pid: process.pid,
          acquiredAt: Date.now(),
        }),
      )
      return async () => {
        await rm(lockDir, { recursive: true, force: true }).catch(() => {})
      }
    } catch {
      const stale = await isLockStale(lockDir)
      if (stale) {
        await rm(lockDir, { recursive: true, force: true }).catch(() => {})
        continue
      }

      await sleep(50)
    }
  }

  throw new Error(`Timed out waiting for worktree sync lock in ${commonDir}`)
}

async function isLockStale(lockDir: string) {
  try {
    const owner = JSON.parse(await readFile(join(lockDir, "owner.json"), "utf-8")) as {
      pid?: number
      acquiredAt?: number
    }
    if (typeof owner.pid === "number" && isProcessAlive(owner.pid)) {
      return false
    }

    return true
  } catch {
    return true
  }
}

function isProcessAlive(pid: number) {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

function sleep(ms: number) {
  return new Promise((resolvePromise) => {
    setTimeout(resolvePromise, ms)
  })
}

async function objectExists(cwd: string, objectRef: string) {
  const result = await runGit(cwd, ["cat-file", "-e", objectRef], { allowFailure: true })
  return result.status === 0
}

async function runGit(
  cwd: string,
  args: string[],
  options: {
    allowFailure?: boolean
    stdin?: "ignore" | string
  } = {},
) {
  const result = await runCommand("git", args, {
    cwd,
    stdin: options.stdin,
  })
  if (result.status !== 0 && options.allowFailure !== true) {
    throw new Error(
      `git ${args.join(" ")} failed in ${cwd}: ${result.stderr.trim() || result.stdout.trim() || "unknown git error"}`,
    )
  }

  return result
}

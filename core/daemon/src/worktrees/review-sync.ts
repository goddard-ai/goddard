/** Review-sync-backed daemon sync host for one primary checkout and session worktree. */
import { mkdir, readdir, readFile, realpath, rm, writeFile } from "node:fs/promises"
import { join, resolve } from "node:path"
import {
  startReviewSync,
  statusReviewSession,
  stopReviewSession,
  type ReviewSyncStatusData,
} from "@goddard-ai/review-sync"
import type { DaemonSessionId } from "@goddard-ai/schema/common/params"

import { runCommand } from "./process.ts"
import type { WorktreeSyncSessionState } from "./sync.ts"

type ReviewSyncHostMetadata = {
  schemaVersion: 1
  sessionId: DaemonSessionId
  primaryDir: string
  worktreeDir: string
  commonDir: string
  agentBranch: string
  baseOid: string
  primaryOriginalHeadOid: string
  primaryOriginalSymbolicRef: string | null
  primaryOriginalBranchTipOid: string | null
  primaryPreMountSnapshotOid: string | null
  mountedAt: number
}

const schemaVersion = 1

const reviewSyncHostRefs = {
  primaryPreMount: (sessionId: DaemonSessionId) =>
    `refs/goddard/review-sync-host/${sessionId}/primary/pre-mount`,
}

/** Scans daemon-owned review-sync metadata for a mounted session targeting one primary checkout. */
export async function findMountedReviewSyncSessionByPrimaryDir(primaryDir: string) {
  const normalizedPrimaryDir = await normalizePath(primaryDir)
  const commonDir = await resolveGitCommonDir(normalizedPrimaryDir)
  if (!commonDir) {
    return null
  }

  let entries: string[]
  try {
    entries = await readdir(resolveMetadataDir(commonDir))
  } catch {
    return null
  }

  for (const entry of entries) {
    if (!entry.endsWith(".json")) {
      continue
    }

    const metadata = await readMetadata(join(resolveMetadataDir(commonDir), entry))
    if (metadata?.primaryDir !== normalizedPrimaryDir) {
      continue
    }

    const state = await createStateFromMetadata(metadata).catch(() => null)
    if (state) {
      return state
    }
  }

  return null
}

/** Adapts daemon mount/inspect/unmount operations to durable review-sync commands. */
export class ReviewSyncWorktreeSessionHost {
  readonly #sessionId
  readonly #primaryDir
  readonly #worktreeDir

  constructor(input: { sessionId: DaemonSessionId; primaryDir: string; worktreeDir: string }) {
    this.#sessionId = input.sessionId
    this.#primaryDir = input.primaryDir
    this.#worktreeDir = input.worktreeDir
  }

  /** Returns the mounted review-sync state when this daemon session still owns one. */
  async inspect() {
    const metadata = await readMetadataForHost({
      primaryDir: this.#primaryDir,
      sessionId: this.#sessionId,
    })
    if (!metadata) {
      return null
    }

    return await createStateFromMetadata(metadata)
  }

  /** Starts or reuses review-sync while preserving the primary checkout's pre-mount state. */
  async mount() {
    const existing = await this.inspect()
    if (existing) {
      return existing
    }

    const metadata = await createMetadata({
      sessionId: this.#sessionId,
      primaryDir: this.#primaryDir,
      worktreeDir: this.#worktreeDir,
    })
    await writeMetadata(metadata)

    try {
      await startReviewSync({
        cwd: metadata.primaryDir,
        agentBranch: metadata.agentBranch,
      })
      const state = await createStateFromMetadata(metadata)
      if (!state) {
        throw new Error("review-sync did not create an inspectable session")
      }
      return state
    } catch (error) {
      await stopReviewSession({ cwd: metadata.worktreeDir }).catch(() => {})
      await restorePrimaryCheckout(metadata).catch(() => {})
      await deleteHostState(metadata).catch(() => {})
      throw error
    }
  }

  /** Stops review-sync and restores the primary checkout state captured before mount. */
  async unmount() {
    const metadata = await readMetadataForHost({
      primaryDir: this.#primaryDir,
      sessionId: this.#sessionId,
    })
    if (!metadata) {
      await stopReviewSession({ cwd: this.#worktreeDir }).catch(() => {})
      return { state: null, warnings: [] }
    }

    await stopReviewSession({ cwd: metadata.worktreeDir })
    const warnings = await restorePrimaryCheckout(metadata)
    await deleteHostState(metadata)

    return {
      state: null,
      warnings,
    }
  }
}

async function createStateFromMetadata(metadata: ReviewSyncHostMetadata) {
  const status = await loadReviewSyncStatus(metadata)
  if (!status) {
    return null
  }

  return {
    sessionId: metadata.sessionId,
    status: "mounted",
    conflictPreference: "worktree",
    primaryDir: status.reviewWorktree,
    worktreeDir: status.agentWorktree,
    commonDir: metadata.commonDir,
    baseOid: metadata.baseOid,
    primaryOriginalHeadOid: metadata.primaryOriginalHeadOid,
    primaryOriginalSymbolicRef: metadata.primaryOriginalSymbolicRef,
    primaryOriginalBranchTipOid: metadata.primaryOriginalBranchTipOid,
    primaryLatestSnapshotOid: status.renderedSnapshot,
    worktreeLatestSnapshotOid: status.agentSnapshot,
    resultSnapshotOid: status.renderedSnapshot,
    primaryRecoverySnapshotOid: null,
    lastSyncAt: null,
  } satisfies WorktreeSyncSessionState
}

async function loadReviewSyncStatus(metadata: ReviewSyncHostMetadata) {
  try {
    const result = await statusReviewSession({
      cwd: metadata.worktreeDir,
      json: true,
    })
    if (!result.data) {
      return null
    }

    return isExpectedReviewSyncSession(metadata, result.data) ? result.data : null
  } catch (error) {
    if (isMissingReviewSyncSessionError(error)) {
      return null
    }
    throw error
  }
}

async function createMetadata(input: {
  sessionId: DaemonSessionId
  primaryDir: string
  worktreeDir: string
}) {
  const primaryDir = await normalizePath(input.primaryDir)
  const worktreeDir = await normalizePath(input.worktreeDir)
  const commonDir = await resolveRequiredCommonDir(primaryDir, worktreeDir)
  const agentBranch = await resolveRequiredCurrentBranch(worktreeDir)
  const primaryOriginalSymbolicRef = await resolveSymbolicRef(primaryDir)
  const primaryOriginalBranchTipOid = primaryOriginalSymbolicRef
    ? await resolveRefOid(primaryDir, primaryOriginalSymbolicRef)
    : null
  const metadata: ReviewSyncHostMetadata = {
    schemaVersion,
    sessionId: input.sessionId,
    primaryDir,
    worktreeDir,
    commonDir,
    agentBranch,
    baseOid: await resolveRequiredHeadOid(worktreeDir),
    primaryOriginalHeadOid: await resolveRequiredHeadOid(primaryDir),
    primaryOriginalSymbolicRef,
    primaryOriginalBranchTipOid,
    primaryPreMountSnapshotOid: null,
    mountedAt: Date.now(),
  }

  metadata.primaryPreMountSnapshotOid = await capturePrimaryPreMountSnapshot(metadata)
  return metadata
}

async function capturePrimaryPreMountSnapshot(metadata: ReviewSyncHostMetadata) {
  const previousTop = await resolveRefOid(metadata.primaryDir, "refs/stash")
  await runGit(
    metadata.primaryDir,
    ["stash", "push", "-u", "-m", `${metadata.sessionId}:pre-mount`],
    {
      allowFailure: true,
    },
  )
  const nextTop = await resolveRefOid(metadata.primaryDir, "refs/stash")
  if (!nextTop || nextTop === previousTop) {
    await setRef(metadata.primaryDir, reviewSyncHostRefs.primaryPreMount(metadata.sessionId), null)
    return null
  }

  await setRef(metadata.primaryDir, reviewSyncHostRefs.primaryPreMount(metadata.sessionId), nextTop)
  await runGit(metadata.primaryDir, ["stash", "drop", "stash@{0}"])
  return nextTop
}

async function restorePrimaryCheckout(metadata: ReviewSyncHostMetadata) {
  const warnings: string[] = []
  await runGit(metadata.primaryDir, ["reset", "--hard"])
  await runGit(metadata.primaryDir, ["clean", "-fd"])

  if (!metadata.primaryOriginalSymbolicRef) {
    await runGit(metadata.primaryDir, ["checkout", "--detach", metadata.primaryOriginalHeadOid], {
      stdin: "ignore",
    })
  } else {
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
    } else {
      await runGit(metadata.primaryDir, ["checkout", "--detach", metadata.primaryOriginalHeadOid], {
        stdin: "ignore",
      })
      warnings.push(
        `Primary branch ${metadata.primaryOriginalSymbolicRef} moved while sync was mounted; restored detached HEAD at the original commit instead.`,
      )
    }
  }

  const snapshotOid =
    metadata.primaryPreMountSnapshotOid ??
    (await resolveRefOid(
      metadata.primaryDir,
      reviewSyncHostRefs.primaryPreMount(metadata.sessionId),
    ))
  if (snapshotOid) {
    await runGit(metadata.primaryDir, ["stash", "apply", "--index", snapshotOid])
  }

  return warnings
}

async function deleteHostState(metadata: ReviewSyncHostMetadata) {
  await setRef(metadata.primaryDir, reviewSyncHostRefs.primaryPreMount(metadata.sessionId), null)
  await rm(resolveMetadataPath(metadata.commonDir, metadata.sessionId), {
    force: true,
  })
}

function isExpectedReviewSyncSession(metadata: ReviewSyncHostMetadata, data: ReviewSyncStatusData) {
  return (
    data.agentBranch === metadata.agentBranch &&
    data.agentWorktree === metadata.worktreeDir &&
    data.reviewWorktree === metadata.primaryDir
  )
}

function isMissingReviewSyncSessionError(error: unknown) {
  return (
    error instanceof Error &&
    error.message.includes("No review-sync session matches the current worktree.")
  )
}

async function readMetadataForHost(input: { primaryDir: string; sessionId: DaemonSessionId }) {
  const primaryDir = await normalizePath(input.primaryDir)
  const commonDir = await resolveGitCommonDir(primaryDir)
  if (!commonDir) {
    return null
  }

  const metadata = await readMetadata(resolveMetadataPath(commonDir, input.sessionId))
  if (metadata?.primaryDir !== primaryDir) {
    return null
  }
  return metadata
}

async function writeMetadata(metadata: ReviewSyncHostMetadata) {
  await mkdir(resolveMetadataDir(metadata.commonDir), { recursive: true })
  await writeFile(
    resolveMetadataPath(metadata.commonDir, metadata.sessionId),
    JSON.stringify(metadata),
  )
}

async function readMetadata(metadataPath: string) {
  try {
    const parsed = JSON.parse(await readFile(metadataPath, "utf-8")) as ReviewSyncHostMetadata
    if (parsed.schemaVersion !== schemaVersion || typeof parsed.sessionId !== "string") {
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

async function resolveRequiredCurrentBranch(cwd: string) {
  const result = await runGit(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"], {
    allowFailure: true,
  })
  const branch = result.status === 0 ? result.stdout.trim() : null
  if (!branch) {
    throw new Error(`Worktree ${cwd} must be on a branch before review-sync can start.`)
  }
  return branch
}

async function resolveRequiredHeadOid(cwd: string) {
  const headOid = await resolveRefOid(cwd, "HEAD")
  if (!headOid) {
    throw new Error(`Failed to resolve HEAD for ${cwd}`)
  }

  return headOid
}

async function resolveSymbolicRef(cwd: string) {
  const result = await runGit(cwd, ["symbolic-ref", "-q", "HEAD"], {
    allowFailure: true,
  })
  return result.status === 0 ? result.stdout.trim() || null : null
}

async function resolveRefOid(cwd: string, refName: string) {
  const result = await runGit(cwd, ["rev-parse", "--verify", "-q", refName], {
    allowFailure: true,
  })
  return result.status === 0 ? result.stdout.trim() || null : null
}

async function resolveGitCommonDir(cwd: string) {
  const result = await runGit(cwd, ["rev-parse", "--git-common-dir"], {
    allowFailure: true,
  })
  const value = result.stdout.trim()
  return value ? await normalizePath(resolve(cwd, value)) : null
}

async function normalizePath(value: string) {
  return await realpath(resolve(value))
}

function resolveMetadataDir(commonDir: string) {
  return join(commonDir, "goddard", "review-sync-host")
}

function resolveMetadataPath(commonDir: string, sessionId: DaemonSessionId) {
  return join(resolveMetadataDir(commonDir), `${sessionId}.json`)
}

async function setRef(cwd: string, refName: string, oid: string | null) {
  if (!oid) {
    await runGit(cwd, ["update-ref", "-d", refName], { allowFailure: true })
    return
  }

  await runGit(cwd, ["update-ref", refName, oid])
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
      `git ${args.join(" ")} failed in ${cwd}: ${
        result.stderr.trim() || result.stdout.trim() || "unknown Git error"
      }`,
    )
  }

  return result
}

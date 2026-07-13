/** Git-backed merge readiness and execution helpers for daemon-owned session worktrees. */
import { realpathSync } from "node:fs"
import { resolve } from "node:path"
import { statusReviewSession, stopReviewSession } from "@goddard-ai/review-sync"

import type { SessionWorktreeMergeReadiness } from "../../schema.ts"
import { resolveGitHeadRef } from "../worktree.ts"
import { runCommand } from "./process.ts"

type MergePrimaryHeadIdentity = {
  headOid: string
  symbolicRef: string | null
  branchTipOid: string | null
}

/**
 * Normalizes one user-provided merge target branch name and verifies that it currently exists.
 */
export async function normalizeAndValidateMergeTargetBranch(params: {
  primaryDir: string
  mergeTargetBranch: string | null
}) {
  if (params.mergeTargetBranch === null) {
    return null
  }

  const normalizedPrimaryDir = normalizeExistingPath(params.primaryDir)
  const normalizedBranch = params.mergeTargetBranch.trim()
  if (!normalizedBranch) {
    throw new Error("Merge target branch must be a non-empty local branch name.")
  }

  const branchFormatResult = await runGit(
    normalizedPrimaryDir,
    ["check-ref-format", "--branch", normalizedBranch],
    { allowFailure: true },
  )
  if (branchFormatResult.status !== 0) {
    throw new Error(`Invalid merge target branch: ${normalizedBranch}`)
  }

  const targetBranchHeadOid = await resolveRefOid(
    normalizedPrimaryDir,
    toLocalBranchRef(normalizedBranch),
  )
  if (!targetBranchHeadOid) {
    throw new Error(`Merge target branch does not exist locally: ${normalizedBranch}`)
  }

  return normalizedBranch
}

/**
 * Evaluates whether one persisted session worktree can merge into its current target branch.
 */
export async function getSessionWorktreeMergeReadiness(params: {
  primaryDir: string
  worktreeDir: string
  mergeTargetBranch: string | null
  sessionActive: boolean
  sessionPrNumber: number | null
}) {
  const readiness = createDefaultReadiness(params.mergeTargetBranch)
  let normalizedPrimaryDir: string
  let normalizedWorktreeDir: string
  try {
    normalizedPrimaryDir = normalizeExistingPath(params.primaryDir)
    normalizedWorktreeDir = normalizeExistingPath(params.worktreeDir)
  } catch {
    return {
      ...readiness,
      status: "worktree_missing",
    } satisfies SessionWorktreeMergeReadiness
  }

  const worktreeHeadOid = await resolveHeadOid(normalizedWorktreeDir)
  if (!worktreeHeadOid) {
    return {
      ...readiness,
      status: "worktree_missing",
    } satisfies SessionWorktreeMergeReadiness
  }

  const syncState = await inspectSyncState(params.worktreeDir)

  if (params.sessionPrNumber !== null) {
    return {
      ...readiness,
      status: "pr_scoped_session",
      syncMounted: syncState.syncMounted,
      willAutoUnmountSync: syncState.willAutoUnmountSync,
    } satisfies SessionWorktreeMergeReadiness
  }

  if (params.sessionActive) {
    return {
      ...readiness,
      status: "session_active",
      syncMounted: syncState.syncMounted,
      willAutoUnmountSync: false,
    } satisfies SessionWorktreeMergeReadiness
  }

  if (params.mergeTargetBranch === null) {
    return {
      ...readiness,
      status: "merge_target_branch_required",
      syncMounted: syncState.syncMounted,
      willAutoUnmountSync: syncState.willAutoUnmountSync,
    } satisfies SessionWorktreeMergeReadiness
  }

  const [targetBranchHeadOid, worktreeHeadBranch] = await Promise.all([
    resolveRefOid(normalizedPrimaryDir, toLocalBranchRef(params.mergeTargetBranch)),
    resolveGitHeadRef(normalizedWorktreeDir).catch(() => null),
  ])

  const nextReadiness = {
    ...readiness,
    worktreeHeadOid,
    worktreeHeadBranch,
    targetBranchHeadOid,
    syncMounted: syncState.syncMounted,
    willAutoUnmountSync: syncState.willAutoUnmountSync,
  }

  if (!targetBranchHeadOid) {
    return {
      ...nextReadiness,
      status: "merge_target_branch_missing",
    } satisfies SessionWorktreeMergeReadiness
  }

  if (await isWorkingTreeDirty(normalizedPrimaryDir)) {
    return {
      ...nextReadiness,
      status: "primary_dirty",
    } satisfies SessionWorktreeMergeReadiness
  }

  if (await isWorkingTreeDirty(normalizedWorktreeDir)) {
    return {
      ...nextReadiness,
      status: "worktree_dirty",
    } satisfies SessionWorktreeMergeReadiness
  }

  const aheadCount = await resolveAheadCount(normalizedWorktreeDir, params.mergeTargetBranch)
  if (aheadCount === 0) {
    return {
      ...nextReadiness,
      status: "not_ahead",
      aheadCount,
    } satisfies SessionWorktreeMergeReadiness
  }

  if (!(await isLocalBranchAncestorOfHead(normalizedWorktreeDir, params.mergeTargetBranch))) {
    return {
      ...nextReadiness,
      status: "not_fast_forward",
      aheadCount,
    } satisfies SessionWorktreeMergeReadiness
  }

  return {
    ...nextReadiness,
    status: "ready",
    aheadCount,
  } satisfies SessionWorktreeMergeReadiness
}

/**
 * Unmounts one mounted review session discovered from the session worktree when present.
 */
export async function unmountMountedWorktreeSync(worktreeDir: string) {
  const reviewSession = await readReviewSessionState(worktreeDir)
  if (!reviewSession) {
    return false
  }

  await stopReviewSession({ cwd: worktreeDir })
  return true
}

/**
 * Fast-forwards one local target branch in the primary checkout to the session worktree HEAD.
 */
export async function mergeSessionWorktreeIntoTarget(params: {
  primaryDir: string
  worktreeDir: string
  mergeTargetBranch: string
}) {
  const normalizedPrimaryDir = normalizeExistingPath(params.primaryDir)
  const normalizedWorktreeDir = normalizeExistingPath(params.worktreeDir)
  const [sourceHeadOid, previousTargetHeadOid, originalPrimaryHead] = await Promise.all([
    resolveRequiredHeadOid(normalizedWorktreeDir),
    resolveRequiredRefOid(normalizedPrimaryDir, toLocalBranchRef(params.mergeTargetBranch)),
    capturePrimaryHeadIdentity(normalizedPrimaryDir),
  ])

  try {
    await runGit(normalizedPrimaryDir, ["checkout", params.mergeTargetBranch], {
      stdin: "ignore",
    })
    await runGit(normalizedPrimaryDir, ["merge", "--ff-only", sourceHeadOid], {
      stdin: "ignore",
    })
  } catch (error) {
    await restorePrimaryHeadIdentity(normalizedPrimaryDir, originalPrimaryHead).catch(() => {})
    throw error
  }

  return {
    targetBranch: params.mergeTargetBranch,
    sourceHeadOid,
    previousTargetHeadOid,
    nextTargetHeadOid: await resolveRequiredRefOid(
      normalizedPrimaryDir,
      toLocalBranchRef(params.mergeTargetBranch),
    ),
  }
}

function createDefaultReadiness(mergeTargetBranch: string | null) {
  return {
    status: "missing_worktree",
    mergeTargetBranch,
    worktreeHeadOid: null,
    worktreeHeadBranch: null,
    targetBranchHeadOid: null,
    aheadCount: 0,
    syncMounted: false,
    willAutoUnmountSync: false,
  } satisfies SessionWorktreeMergeReadiness
}

async function inspectSyncState(worktreeDir: string) {
  const reviewSession = await readReviewSessionState(worktreeDir)
  if (!reviewSession) {
    return {
      syncMounted: false,
      willAutoUnmountSync: false,
    }
  }

  return {
    syncMounted: true,
    willAutoUnmountSync: true,
  }
}

async function readReviewSessionState(worktreeDir: string) {
  try {
    const result = await statusReviewSession({
      cwd: worktreeDir,
      json: true,
    })
    return result.data ?? null
  } catch (error) {
    if (
      error instanceof Error &&
      error.message.includes("No review-sync session matches the current worktree.")
    ) {
      return null
    }

    throw error
  }
}

async function capturePrimaryHeadIdentity(primaryDir: string) {
  const [headOid, symbolicRef] = await Promise.all([
    resolveRequiredHeadOid(primaryDir),
    resolveSymbolicRef(primaryDir),
  ])

  return {
    headOid,
    symbolicRef,
    branchTipOid: symbolicRef ? await resolveRefOid(primaryDir, symbolicRef) : null,
  } satisfies MergePrimaryHeadIdentity
}

async function restorePrimaryHeadIdentity(
  primaryDir: string,
  originalHead: MergePrimaryHeadIdentity,
) {
  if (!originalHead.symbolicRef) {
    await runGit(primaryDir, ["checkout", "--detach", originalHead.headOid], { stdin: "ignore" })
    return
  }

  const currentTip = await resolveRefOid(primaryDir, originalHead.symbolicRef)
  if (currentTip && currentTip === originalHead.branchTipOid) {
    await runGit(primaryDir, ["checkout", originalHead.symbolicRef.replace(/^refs\/heads\//, "")], {
      stdin: "ignore",
    })
    return
  }

  await runGit(primaryDir, ["checkout", "--detach", originalHead.headOid], { stdin: "ignore" })
}

async function resolveAheadCount(cwd: string, mergeTargetBranch: string) {
  const result = await runGit(cwd, [
    "rev-list",
    "--count",
    `${toLocalBranchRef(mergeTargetBranch)}..HEAD`,
  ])
  return Number.parseInt(result.stdout.trim() || "0", 10)
}

async function isLocalBranchAncestorOfHead(cwd: string, mergeTargetBranch: string) {
  const result = await runGit(
    cwd,
    ["merge-base", "--is-ancestor", toLocalBranchRef(mergeTargetBranch), "HEAD"],
    { allowFailure: true },
  )
  return result.status === 0
}

async function isWorkingTreeDirty(cwd: string) {
  const result = await runGit(cwd, ["status", "--porcelain"], { allowFailure: true })
  return result.stdout.trim().length > 0
}

async function resolveRequiredHeadOid(cwd: string) {
  const headOid = await resolveHeadOid(cwd)
  if (!headOid) {
    throw new Error(`Failed to resolve HEAD for ${cwd}`)
  }

  return headOid
}

async function resolveHeadOid(cwd: string) {
  const result = await runGit(cwd, ["rev-parse", "HEAD"], { allowFailure: true })
  return result.stdout.trim() || null
}

async function resolveRequiredRefOid(cwd: string, refName: string) {
  const refOid = await resolveRefOid(cwd, refName)
  if (!refOid) {
    throw new Error(`Failed to resolve ref ${refName} for ${cwd}`)
  }

  return refOid
}

async function resolveRefOid(cwd: string, refName: string) {
  const result = await runGit(cwd, ["rev-parse", "--verify", "-q", refName], { allowFailure: true })
  return result.stdout.trim() || null
}

async function resolveSymbolicRef(cwd: string) {
  const result = await runGit(cwd, ["symbolic-ref", "-q", "HEAD"], { allowFailure: true })
  return result.stdout.trim() || null
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
  }).catch((error) => {
    if (options.allowFailure) {
      return {
        status: 1,
        stdout: "",
        stderr: error instanceof Error ? error.message : String(error),
      }
    }

    throw error
  })

  if (!options.allowFailure && result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || `git ${args.join(" ")} failed`)
  }

  return result
}

function normalizeExistingPath(value: string) {
  return resolve(realpathSync.native(resolve(value)))
}

function toLocalBranchRef(branchName: string) {
  return `refs/heads/${branchName}`
}

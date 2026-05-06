/** Thin daemon adapter for review-sync-owned worktree synchronization. */
import { realpath } from "node:fs/promises"
import { resolve } from "node:path"
import {
  listReviewSessions,
  startReviewSync,
  statusReviewSession,
  stopReviewSession,
  type ReviewSyncStatusData,
} from "@goddard-ai/review-sync"

/** Mounted daemon host state mirrors review-sync's native status payload. */
export type WorktreeSyncSessionState = ReviewSyncStatusData

/** Explicit daemon worktree pair passed to each review-sync adapter operation. */
export type WorktreeSyncSessionInput = {
  primaryDir: string
  worktreeDir: string
  agentBranch: string
}

/** Finds the mounted worktree sync session for one daemon primary checkout. */
export async function findMountedWorktreeSyncSessionByPrimaryDir(primaryDir: string) {
  const normalizedPrimaryDir = await normalizePath(primaryDir)
  const sessions = await listReviewSessions({
    cwd: normalizedPrimaryDir,
  })
  return sessions.find((session) => session.reviewWorktree === normalizedPrimaryDir) ?? null
}

/** Starts or reuses worktree sync for the daemon primary checkout and session worktree. */
export async function mountWorktreeSyncSession(input: WorktreeSyncSessionInput) {
  const existing = await loadWorktreeSyncSessionState(input)
  if (existing) {
    return existing
  }

  await startReviewSync({
    cwd: input.primaryDir,
    agentBranch: input.agentBranch,
  })
  const state = await loadWorktreeSyncSessionState(input)
  if (!state) {
    throw new Error("worktree sync did not create a readable session")
  }
  return state
}

/** Stops worktree sync ownership while leaving checkout semantics to the sync implementation. */
export async function unmountWorktreeSyncSession(input: WorktreeSyncSessionInput) {
  await stopReviewSession({
    cwd: input.worktreeDir,
  })

  return {
    state: null,
    warnings: [],
  }
}

async function loadWorktreeSyncSessionState(input: WorktreeSyncSessionInput) {
  const expected = {
    primaryDir: await normalizePath(input.primaryDir),
    worktreeDir: await normalizePath(input.worktreeDir),
    agentBranch: input.agentBranch,
  }

  try {
    const result = await statusReviewSession({
      cwd: expected.worktreeDir,
      json: true,
    })
    if (!result.data) {
      return null
    }

    return isExpectedWorktreeSyncSession(expected, result.data) ? result.data : null
  } catch (error) {
    if (isMissingWorktreeSyncSessionError(error)) {
      return null
    }
    throw error
  }
}

function isExpectedWorktreeSyncSession(
  expected: {
    primaryDir: string
    worktreeDir: string
    agentBranch: string
  },
  data: ReviewSyncStatusData,
) {
  return (
    data.agentBranch === expected.agentBranch &&
    data.agentWorktree === expected.worktreeDir &&
    data.reviewWorktree === expected.primaryDir
  )
}

function isMissingWorktreeSyncSessionError(error: unknown) {
  return (
    error instanceof Error &&
    error.message.includes("No review-sync session matches the current worktree.")
  )
}

async function normalizePath(value: string) {
  return await realpath(resolve(value))
}

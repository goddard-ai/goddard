/** Thin daemon adapter for review session worktree synchronization. */
import { realpath } from "node:fs/promises"
import { resolve } from "node:path"
import {
  listReviewSessions,
  startReviewSync,
  statusReviewSession,
  stopReviewSession,
  type ReviewSyncStatusData,
} from "@goddard-ai/review-sync"

/** Mounted review session state mirrors review-sync's native status payload. */
export type ReviewSessionState = ReviewSyncStatusData

/** Explicit daemon worktree pair passed to each review-sync adapter operation. */
export type ReviewSessionInput = {
  primaryDir: string
  worktreeDir: string
  agentBranch: string
}

/** Finds the mounted review session for one daemon primary checkout. */
export async function findMountedReviewSessionByPrimaryDir(primaryDir: string) {
  const normalizedPrimaryDir = await normalizePath(primaryDir)
  const sessions = await listReviewSessions({
    cwd: normalizedPrimaryDir,
  })
  return sessions.find((session) => session.reviewWorktree === normalizedPrimaryDir) ?? null
}

/** Starts or reuses a review session for the daemon primary checkout and session worktree. */
export async function mountReviewSession(input: ReviewSessionInput) {
  const existing = await loadReviewSessionState(input)
  if (existing) {
    return existing
  }

  await startReviewSync({
    cwd: input.primaryDir,
    agentBranch: input.agentBranch,
  })
  const state = await loadReviewSessionState(input)
  if (!state) {
    throw new Error("review session did not create readable state")
  }
  return state
}

/** Stops review session ownership while leaving checkout semantics to the sync implementation. */
export async function unmountReviewSession(input: ReviewSessionInput) {
  await stopReviewSession({
    cwd: input.worktreeDir,
  })

  return {
    state: null,
    warnings: [],
  }
}

async function loadReviewSessionState(input: ReviewSessionInput) {
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

    return isExpectedReviewSession(expected, result.data) ? result.data : null
  } catch (error) {
    if (isMissingReviewSessionError(error)) {
      return null
    }
    throw error
  }
}

function isExpectedReviewSession(
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

function isMissingReviewSessionError(error: unknown) {
  return (
    error instanceof Error &&
    error.message.includes("No review-sync session matches the current worktree.")
  )
}

async function normalizePath(value: string) {
  return await realpath(resolve(value))
}

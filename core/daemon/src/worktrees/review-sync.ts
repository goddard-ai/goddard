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
export type ReviewSyncWorktreeSessionState = ReviewSyncStatusData

/** Finds the review-sync session whose review worktree is one daemon primary checkout. */
export async function findMountedReviewSyncSessionByPrimaryDir(primaryDir: string) {
  const normalizedPrimaryDir = await normalizePath(primaryDir)
  const sessions = await listReviewSessions({
    cwd: normalizedPrimaryDir,
  })
  return sessions.find((session) => session.reviewWorktree === normalizedPrimaryDir) ?? null
}

/** Adapts daemon mount/inspect/unmount calls to review-sync commands without owning Git state. */
export class ReviewSyncWorktreeSessionHost {
  readonly #primaryDir
  readonly #worktreeDir
  readonly #agentBranch

  constructor(input: { primaryDir: string; worktreeDir: string; agentBranch: string }) {
    this.#primaryDir = input.primaryDir
    this.#worktreeDir = input.worktreeDir
    this.#agentBranch = input.agentBranch
  }

  /** Returns the mounted review-sync state when this daemon worktree pair is active. */
  async inspect() {
    return await loadExpectedReviewSyncStatus({
      primaryDir: this.#primaryDir,
      worktreeDir: this.#worktreeDir,
      agentBranch: this.#agentBranch,
    })
  }

  /** Starts or reuses review-sync for the daemon primary checkout and session worktree. */
  async mount() {
    const existing = await this.inspect()
    if (existing) {
      return existing
    }

    await startReviewSync({
      cwd: this.#primaryDir,
      agentBranch: this.#agentBranch,
    })
    const state = await this.inspect()
    if (!state) {
      throw new Error("review-sync did not create an inspectable session")
    }
    return state
  }

  /** Stops review-sync ownership while leaving checkout semantics to review-sync. */
  async unmount() {
    await stopReviewSession({
      cwd: this.#worktreeDir,
    })

    return {
      state: null,
      warnings: [],
    }
  }
}

async function loadExpectedReviewSyncStatus(input: {
  primaryDir: string
  worktreeDir: string
  agentBranch: string
}) {
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

    return isExpectedReviewSyncSession(expected, result.data) ? result.data : null
  } catch (error) {
    if (isMissingReviewSyncSessionError(error)) {
      return null
    }
    throw error
  }
}

function isExpectedReviewSyncSession(
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

function isMissingReviewSyncSessionError(error: unknown) {
  return (
    error instanceof Error &&
    error.message.includes("No review-sync session matches the current worktree.")
  )
}

async function normalizePath(value: string) {
  return await realpath(resolve(value))
}

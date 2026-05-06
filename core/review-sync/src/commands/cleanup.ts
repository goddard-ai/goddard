/** Cleanup command implementation for review-sync. */
import { command, flag } from "cmd-ts"

import { createReviewSyncResult } from "../errors.ts"
import {
  git,
  isInsideOrEqual,
  resolveRef,
  resolveRequiredGitCommonDir,
  resolveRequiredRepoRoot,
} from "../git.ts"
import { withSessionLock } from "../lock.ts"
import { createRuntimeContext } from "../runtime.ts"
import { deleteSessionState, listSessions } from "../state.ts"
import type { CleanupReviewSyncInput, RuntimeContext, SessionState } from "../types.ts"

/** Removes saved session state records that match the current worktree root. */
export async function cleanupReviewSessions(input: CleanupReviewSyncInput) {
  const context = createRuntimeContext(input.cwd)
  const all = input.all ?? false
  const { resolvedDirectory, sessions } = await listSessionsForResolvedDirectory(context)
  const ordered = [...sessions].sort(compareSessionsByRecency)
  const kept = all ? null : (ordered.at(-1) ?? null)
  const removed = all ? ordered : ordered.slice(0, -1)

  for (const session of removed) {
    await deleteSavedSession(session, resolvedDirectory, context)
  }

  return createReviewSyncResult({
    exitCode: 0,
    command: "cleanup",
    status: "ok",
    sessionId: kept?.sessionId,
    reviewBranch: kept?.reviewBranch,
    message: formatCleanupMessage({
      all,
      resolvedDirectory,
      removed,
      kept,
      matchedCount: ordered.length,
    }),
  })
}

/** Lists saved sessions whose recorded agent or review worktree is the current worktree root. */
async function listSessionsForResolvedDirectory(context: RuntimeContext) {
  const resolvedDirectory = await resolveRequiredRepoRoot(context.cwd, context)
  const repoCommonDir = await resolveRequiredGitCommonDir(resolvedDirectory, context)
  const sessions = (await listSessions(repoCommonDir)).filter(
    (session) =>
      isInsideOrEqual(session.agentWorktree, resolvedDirectory) ||
      isInsideOrEqual(session.reviewWorktree, resolvedDirectory),
  )
  return { resolvedDirectory, sessions }
}

/** Orders sessions from oldest to newest using durable state timestamps. */
function compareSessionsByRecency(left: SessionState, right: SessionState) {
  const leftTimestamp = sessionTimestamp(left)
  const rightTimestamp = sessionTimestamp(right)
  if (leftTimestamp !== rightTimestamp) {
    return leftTimestamp - rightTimestamp
  }
  return left.sessionId.localeCompare(right.sessionId)
}

/** Converts session timestamps to comparable numbers, tolerating malformed stale state. */
function sessionTimestamp(session: SessionState) {
  const updatedAt = Date.parse(session.updatedAt)
  if (!Number.isNaN(updatedAt)) {
    return updatedAt
  }
  const createdAt = Date.parse(session.createdAt)
  return Number.isNaN(createdAt) ? 0 : createdAt
}

/** Deletes one saved session directory and its hidden review-sync refs. */
async function deleteSavedSession(
  session: SessionState,
  resolvedDirectory: string,
  context: RuntimeContext,
) {
  await withSessionLock(session, async () => {
    await Promise.all(
      Object.values(session.refs).map((refName) =>
        deleteSessionRefIfPresent(resolvedDirectory, refName, context),
      ),
    )
    await deleteSessionState(session)
  })
}

/** Removes a hidden session ref when it still exists. */
async function deleteSessionRefIfPresent(
  resolvedDirectory: string,
  refName: string,
  context: RuntimeContext,
) {
  if (await resolveRef(resolvedDirectory, refName, context)) {
    await git(resolvedDirectory, ["update-ref", "-d", refName], context)
  }
}

/** Formats cleanup output without exposing hidden filesystem details by default. */
function formatCleanupMessage(input: {
  all: boolean
  resolvedDirectory: string
  removed: SessionState[]
  kept: SessionState | null
  matchedCount: number
}) {
  if (input.matchedCount === 0) {
    return `No review-sync sessions match ${input.resolvedDirectory}.`
  }

  if (input.removed.length === 0) {
    return `No stale review-sync sessions to remove for ${input.resolvedDirectory}. Kept ${formatSessionSummary(input.kept!)}.`
  }

  const removed = `${input.removed.length} review-sync ${
    input.removed.length === 1 ? "session" : "sessions"
  }`
  if (input.all || !input.kept) {
    return `Removed ${removed} for ${input.resolvedDirectory}.`
  }
  return `Removed ${removed} for ${input.resolvedDirectory}. Kept ${formatSessionSummary(input.kept)}.`
}

/** Produces a compact human summary for cleanup output. */
function formatSessionSummary(session: SessionState) {
  return `${session.sessionId}: ${session.agentBranch} -> ${session.reviewBranch}`
}

/** Builds the cleanup subcommand. */
export function createCleanupCommand(cwd: string) {
  return command({
    name: "cleanup",
    description: "Remove saved review-sync sessions matching the current worktree",
    args: {
      all: flag({
        long: "all",
        short: "A",
        description: "Remove every matching session instead of keeping the newest",
      }),
    },
    handler: ({ all }) => cleanupReviewSessions({ cwd, all }),
  })
}

/** Status command implementation for review-sync. */
import { join } from "node:path"
import { command, flag } from "cmd-ts"

import { createReviewSyncResult } from "../errors.ts"
import { resolveRef, resolveRequiredGitCommonDir, resolveRequiredRepoRoot } from "../git.ts"
import { createRuntimeContext } from "../runtime.ts"
import { inferSession } from "../session.ts"
import { countPatchFiles, listSessions, resolveSessionDir } from "../state.ts"
import type {
  ListReviewSyncInput,
  ReviewSyncStatusData,
  RuntimeContext,
  SessionState,
  StatusReviewSyncInput,
} from "../types.ts"

/** Returns session state, patch counts, and refs without mutating Git or durable state. */
export async function statusReviewSession(input: StatusReviewSyncInput) {
  const context = createRuntimeContext(input.cwd)
  const json = input.json ?? false
  const session = await inferSession(context)
  const payload = await createReviewSyncStatusData(session, context)
  const message = json ? JSON.stringify(payload, null, 2) : formatStatusMessage(payload)

  return createReviewSyncResult({
    exitCode: 0,
    command: "status",
    status: session.paused ? "paused" : "ok",
    sessionId: session.sessionId,
    reviewBranch: session.reviewBranch,
    data: payload,
    message,
  })
}

/** Lists every review-sync session recorded for the current Git repository. */
export async function listReviewSessions(input: ListReviewSyncInput) {
  const context = createRuntimeContext(input.cwd)
  const repoRoot = await resolveRequiredRepoRoot(input.cwd, context)
  const commonDir = await resolveRequiredGitCommonDir(repoRoot, context)
  const sessions = await listSessions(commonDir)
  return await Promise.all(sessions.map((session) => createReviewSyncStatusData(session, context)))
}

/** Builds the status payload shared by direct status and session listing callers. */
async function createReviewSyncStatusData(session: SessionState, context: RuntimeContext) {
  const sessionDir = resolveSessionDir(session.repoCommonDir, session.sessionId)
  const acceptedCount = await countPatchFiles(join(sessionDir, "patches", "accepted"))
  const rejectedCount = await countPatchFiles(join(sessionDir, "patches", "rejected"))
  const agentSnapshot = await resolveRef(session.agentWorktree, session.refs.agentSnapshot, context)
  const renderedSnapshot = await resolveRef(
    session.agentWorktree,
    session.refs.renderedSnapshot,
    context,
  )
  const payload = {
    sessionId: session.sessionId,
    agentWorktree: session.agentWorktree,
    reviewWorktree: session.reviewWorktree,
    agentBranch: session.agentBranch,
    reviewBranch: session.reviewBranch,
    paused: session.paused,
    refs: {
      agentSnapshot: session.refs.agentSnapshot,
      renderedSnapshot: session.refs.renderedSnapshot,
    },
    agentSnapshot,
    renderedSnapshot,
    lastSync: session.lastSync,
    patchCounts: {
      accepted: acceptedCount,
      rejected: rejectedCount,
    },
  } satisfies ReviewSyncStatusData
  return payload
}

function formatStatusMessage(payload: ReviewSyncStatusData) {
  return [
    `review sync: ${payload.agentBranch} -> ${payload.reviewBranch}`,
    `session: ${payload.sessionId}`,
    `paused: ${payload.paused ? "yes" : "no"}`,
    `agent worktree: ${payload.agentWorktree}`,
    `review worktree: ${payload.reviewWorktree}`,
    `agent snapshot: ${payload.agentSnapshot ?? "(none)"}`,
    `rendered snapshot: ${payload.renderedSnapshot ?? "(none)"}`,
    `last sync: ${payload.lastSync.status}`,
    `accepted patches: ${payload.patchCounts.accepted}`,
    `rejected patches: ${payload.patchCounts.rejected}`,
  ].join("\n")
}

/** Builds the status subcommand. */
export function createStatusCommand(cwd: string) {
  return command({
    name: "status",
    description: "Show review-sync session state without mutating Git",
    args: {
      json: flag({
        long: "json",
        description: "Print status as JSON for machine consumers",
      }),
    },
    handler: ({ json }) => statusReviewSession({ cwd, json }),
  })
}

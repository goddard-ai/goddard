/** Status command implementation for review-sync. */
import { join } from "node:path"
import { command, flag } from "cmd-ts"

import { createReviewSyncResult } from "../errors.ts"
import { resolveRef } from "../git.ts"
import { createRuntimeContext } from "../runtime.ts"
import { inferSession } from "../session.ts"
import { countPatchFiles, resolveSessionDir } from "../state.ts"
import type { StatusReviewSyncInput } from "../types.ts"

/** Returns session state, patch counts, and refs without mutating Git or durable state. */
export async function statusReviewSession(input: StatusReviewSyncInput) {
  const context = createRuntimeContext(input.cwd)
  const json = input.json ?? false
  const session = await inferSession(context)
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
    agentSnapshot,
    renderedSnapshot,
    lastSync: session.lastSync,
    patchCounts: {
      accepted: acceptedCount,
      rejected: rejectedCount,
    },
  }
  const message = json
    ? JSON.stringify(payload, null, 2)
    : [
        `review sync: ${session.agentBranch} -> ${session.reviewBranch}`,
        `session: ${session.sessionId}`,
        `paused: ${session.paused ? "yes" : "no"}`,
        `agent worktree: ${session.agentWorktree}`,
        `review worktree: ${session.reviewWorktree}`,
        `agent snapshot: ${agentSnapshot ?? "(none)"}`,
        `rendered snapshot: ${renderedSnapshot ?? "(none)"}`,
        `last sync: ${session.lastSync.status}`,
        `accepted patches: ${acceptedCount}`,
        `rejected patches: ${rejectedCount}`,
      ].join("\n")

  return createReviewSyncResult({
    exitCode: 0,
    command: "status",
    status: session.paused ? "paused" : "ok",
    sessionId: session.sessionId,
    reviewBranch: session.reviewBranch,
    message,
  })
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

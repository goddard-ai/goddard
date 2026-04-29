/** CLI-compatible review-sync command parsing and dispatch. */
import { join } from "node:path"

import { createErrorResult, createReviewSyncResult, UserError } from "./errors.ts"
import { resolveRef } from "./git.ts"
import { withSessionLock } from "./lock.ts"
import { createRuntimeContext, writeResult } from "./runtime.ts"
import { createSessionForStart, inferSession, prepareReviewBranchForStart } from "./session.ts"
import {
  appendEvent,
  countPatchFiles,
  readSessionState,
  resolveSessionDir,
  writeSessionState,
} from "./state.ts"
import { syncSession } from "./sync.ts"
import type { ParsedCommand, ReviewSyncEnv, RuntimeContext } from "./types.ts"

/**
 * Runs one review-sync command using the same command names and arguments as the CLI.
 */
export async function runReviewSync(argv: string[], env: ReviewSyncEnv = {}) {
  const command = parseCommandName(argv)
  const context = createRuntimeContext(env)

  try {
    const parsed = parseCommand(argv)
    const result = await executeCommand(parsed, context)
    writeResult(context, result)
    return result
  } catch (error) {
    const result = createErrorResult(command, error)
    writeResult(context, result)
    return result
  }
}

/** Dispatches one parsed command to the bounded operation that owns its mutations. */
async function executeCommand(parsed: ParsedCommand, context: RuntimeContext) {
  switch (parsed.command) {
    case "start":
      return await startReviewSync(parsed.reviewWorktree, context)
    case "sync":
      return await syncReviewSession(context)
    case "status":
      return await statusReviewSession(parsed.json, context)
    case "pause":
      return await pauseReviewSession(context)
    case "resume":
      return await resumeReviewSession(context)
  }
}

/** Creates or reuses one durable review-sync session and runs the first refresh. */
async function startReviewSync(reviewWorktreeInput: string, context: RuntimeContext) {
  const session = await createSessionForStart(reviewWorktreeInput, context)
  await prepareReviewBranchForStart(session, context)
  const syncResult = await syncSession(session, context)

  return createReviewSyncResult({
    exitCode: 0,
    command: "start",
    status: syncResult.status === "rejected-human-patch" ? "rejected-human-patch" : "ok",
    sessionId: session.sessionId,
    reviewBranch: session.reviewBranch,
    acceptedPatchPath: syncResult.acceptedPatchPath ?? undefined,
    rejectedPatchPath: syncResult.rejectedPatchPath ?? undefined,
    message:
      syncResult.status === "rejected-human-patch"
        ? `Started review sync for ${session.agentBranch} as ${session.reviewBranch}; human patch was rejected and saved to ${syncResult.rejectedPatchPath}.`
        : `Started review sync for ${session.agentBranch} as ${session.reviewBranch}.`,
  })
}

/** Runs one sync operation for the session inferred from the current worktree. */
async function syncReviewSession(context: RuntimeContext) {
  const session = await inferSession(context)
  const syncResult = await syncSession(session, context)

  return createReviewSyncResult({
    exitCode: 0,
    command: "sync",
    status: syncResult.status === "rejected-human-patch" ? "rejected-human-patch" : "ok",
    sessionId: session.sessionId,
    reviewBranch: session.reviewBranch,
    acceptedPatchPath: syncResult.acceptedPatchPath ?? undefined,
    rejectedPatchPath: syncResult.rejectedPatchPath ?? undefined,
    message:
      syncResult.status === "rejected-human-patch"
        ? `Human patch rejected and saved to ${syncResult.rejectedPatchPath}. Review branch refreshed from ${session.agentBranch}.`
        : `Synced ${session.agentBranch} to ${session.reviewBranch}.`,
  })
}

/** Returns session state, patch counts, and refs without mutating Git or durable state. */
async function statusReviewSession(json: boolean, context: RuntimeContext) {
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

/** Marks the inferred session paused so later sync commands refuse to mutate it. */
async function pauseReviewSession(context: RuntimeContext) {
  const session = await inferSession(context)
  return await withSessionLock(session, context, async () => {
    const latest = await readSessionState(session)
    latest.paused = true
    latest.updatedAt = new Date().toISOString()
    latest.lastSync = {
      status: "paused",
      acceptedPatch: null,
      rejectedPatch: null,
    }
    await writeSessionState(latest)
    await appendEvent(latest, {
      command: "pause",
      status: "paused",
    })

    return createReviewSyncResult({
      exitCode: 0,
      command: "pause",
      status: "paused",
      sessionId: latest.sessionId,
      reviewBranch: latest.reviewBranch,
      message: `Paused review sync for ${latest.reviewBranch}.`,
    })
  })
}

/** Clears the paused flag without running an implicit sync. */
async function resumeReviewSession(context: RuntimeContext) {
  const session = await inferSession(context)
  return await withSessionLock(session, context, async () => {
    const latest = await readSessionState(session)
    latest.paused = false
    latest.updatedAt = new Date().toISOString()
    await writeSessionState(latest)
    await appendEvent(latest, {
      command: "resume",
      status: "ok",
    })

    return createReviewSyncResult({
      exitCode: 0,
      command: "resume",
      status: "ok",
      sessionId: latest.sessionId,
      reviewBranch: latest.reviewBranch,
      message: `Resumed review sync for ${latest.reviewBranch}.`,
    })
  })
}

/** Parses CLI arguments into one supported review-sync command. */
function parseCommand(argv: string[]) {
  const [command, ...rest] = argv
  switch (command) {
    case "start": {
      const reviewWorktreeIndex = rest.indexOf("--review-worktree")
      if (reviewWorktreeIndex === -1 || !rest[reviewWorktreeIndex + 1]) {
        throw new UserError("Usage: review-sync start --review-worktree <path>")
      }
      if (rest.length !== 2) {
        throw new UserError("Usage: review-sync start --review-worktree <path>")
      }
      return {
        command: "start",
        reviewWorktree: rest[reviewWorktreeIndex + 1]!,
      } satisfies ParsedCommand
    }
    case "sync":
      if (rest.length > 0) {
        throw new UserError("Usage: review-sync sync")
      }
      return { command: "sync" } satisfies ParsedCommand
    case "status":
      if (rest.some((arg) => arg !== "--json")) {
        throw new UserError("Usage: review-sync status [--json]")
      }
      return { command: "status", json: rest.includes("--json") } satisfies ParsedCommand
    case "pause":
      if (rest.length > 0) {
        throw new UserError("Usage: review-sync pause")
      }
      return { command: "pause" } satisfies ParsedCommand
    case "resume":
      if (rest.length > 0) {
        throw new UserError("Usage: review-sync resume")
      }
      return { command: "resume" } satisfies ParsedCommand
    default:
      throw new UserError("Usage: review-sync <start|sync|status|pause|resume> [command options]")
  }
}

/** Extracts a best-effort command name for structured error results. */
function parseCommandName(argv: string[]) {
  const command = argv[0]
  return command === "start" ||
    command === "sync" ||
    command === "status" ||
    command === "pause" ||
    command === "resume"
    ? command
    : "status"
}

/** CLI-compatible review-sync command runner. */
import { join } from "node:path"
import { command, flag, option, runSafely, string, subcommands } from "cmd-ts"

import { createErrorResult, createReviewSyncResult } from "./errors.ts"
import { resolveRef } from "./git.ts"
import { withSessionLock } from "./lock.ts"
import { createRuntimeContext } from "./runtime.ts"
import { createSessionForStart, inferSession, prepareReviewBranchForStart } from "./session.ts"
import {
  appendEvent,
  countPatchFiles,
  readSessionState,
  resolveSessionDir,
  writeSessionState,
} from "./state.ts"
import { syncSession } from "./sync.ts"
import type {
  ReviewSyncCommand,
  ReviewSyncResult,
  ReviewSyncWorktreeInput,
  RuntimeContext,
  StartReviewSyncInput,
  StatusReviewSyncInput,
} from "./types.ts"

/**
 * Runs one review-sync command using the same command names and process context as the CLI.
 */
export async function runReviewSync(argv: string[]) {
  try {
    const parsed = await runSafely(createReviewSyncCommand(process.cwd()), argv)
    return parsed._tag === "error" ? createCmdTsResult(parsed.error) : await parsed.value.value
  } catch (error) {
    return createErrorResult("status", error)
  }
}

/** Creates or reuses one durable review-sync session and runs the first refresh. */
export async function startReviewSync(input: StartReviewSyncInput) {
  return await runCommandSafely("start", () =>
    startReviewSyncOperation(input.reviewWorktree, createRuntimeContext(input.cwd)),
  )
}

/** Runs one sync operation for the session inferred from the current worktree. */
export async function syncReviewSession(input: ReviewSyncWorktreeInput) {
  return await runCommandSafely("sync", () =>
    syncReviewSessionOperation(createRuntimeContext(input.cwd)),
  )
}

/** Returns session state, patch counts, and refs without mutating Git or durable state. */
export async function statusReviewSession(input: StatusReviewSyncInput) {
  return await runCommandSafely("status", () =>
    statusReviewSessionOperation(input.json ?? false, createRuntimeContext(input.cwd)),
  )
}

/** Marks the inferred session paused so later sync commands refuse to mutate it. */
export async function pauseReviewSession(input: ReviewSyncWorktreeInput) {
  return await runCommandSafely("pause", () =>
    pauseReviewSessionOperation(createRuntimeContext(input.cwd)),
  )
}

/** Clears the paused flag without running an implicit sync. */
export async function resumeReviewSession(input: ReviewSyncWorktreeInput) {
  return await runCommandSafely("resume", () =>
    resumeReviewSessionOperation(createRuntimeContext(input.cwd)),
  )
}

/** Performs the start workflow after CLI parsing and command-level error handling. */
async function startReviewSyncOperation(reviewWorktreeInput: string, context: RuntimeContext) {
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

/** Performs the sync workflow after CLI parsing and command-level error handling. */
async function syncReviewSessionOperation(context: RuntimeContext) {
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

/** Performs the status workflow after CLI parsing and command-level error handling. */
async function statusReviewSessionOperation(json: boolean, context: RuntimeContext) {
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

/** Performs the pause workflow after CLI parsing and command-level error handling. */
async function pauseReviewSessionOperation(context: RuntimeContext) {
  const session = await inferSession(context)
  return await withSessionLock(session, async () => {
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

/** Performs the resume workflow after CLI parsing and command-level error handling. */
async function resumeReviewSessionOperation(context: RuntimeContext) {
  const session = await inferSession(context)
  return await withSessionLock(session, async () => {
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

/** Preserves command-specific structured errors while letting cmd-ts route subcommands. */
async function runCommandSafely(
  command: ReviewSyncCommand,
  operation: () => Promise<ReviewSyncResult>,
) {
  try {
    return await operation()
  } catch (error) {
    return createErrorResult(command, error)
  }
}

/** Converts cmd-ts parse/help exits into the public structured result shape. */
function createCmdTsResult(error: { config: { exitCode: number; message: string } }) {
  return createReviewSyncResult({
    exitCode: error.config.exitCode,
    command: "status",
    status: error.config.exitCode === 0 ? "ok" : "error",
    message: error.config.message,
  })
}

/** Builds the command tree with handlers that execute the bounded operations directly. */
function createReviewSyncCommand(cwd: string) {
  return subcommands({
    name: "review-sync",
    description: "Synchronize an agent-owned branch with a disposable human review branch",
    cmds: {
      start: command({
        name: "start",
        description: "Create or reuse a review branch and run the initial sync",
        args: {
          reviewWorktree: option({
            type: string,
            long: "review-worktree",
            description: "Path to the local worktree where the human review branch is checked out",
          }),
        },
        handler: ({ reviewWorktree }) => startReviewSync({ cwd, reviewWorktree }),
      }),
      sync: command({
        name: "sync",
        description: "Apply clean human edits to the agent worktree and refresh the review branch",
        args: {},
        handler: () => syncReviewSession({ cwd }),
      }),
      status: command({
        name: "status",
        description: "Show review-sync session state without mutating Git",
        args: {
          json: flag({
            long: "json",
            description: "Print status as JSON for machine consumers",
          }),
        },
        handler: ({ json }) => statusReviewSession({ cwd, json }),
      }),
      pause: command({
        name: "pause",
        description: "Pause future sync mutations for the inferred session",
        args: {},
        handler: () => pauseReviewSession({ cwd }),
      }),
      resume: command({
        name: "resume",
        description: "Resume sync mutations without running an immediate sync",
        args: {},
        handler: () => resumeReviewSession({ cwd }),
      }),
    },
  })
}

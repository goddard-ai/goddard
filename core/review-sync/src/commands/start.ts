/** Start command implementation for review-sync. */
import { autocomplete, cancel, isCancel } from "@clack/prompts"
import { command, optional, positional, string } from "cmd-ts"

import { createReviewSyncResult, UserError } from "../errors.ts"
import { isWorktreeClean, resolveCurrentBranch, resolveRef, updateRef } from "../git.ts"
import { createRuntimeContext } from "../runtime.ts"
import {
  createSessionForStart,
  listAgentWorktreeChoicesForStart,
  prepareReviewBranchForStart,
} from "../session.ts"
import { readSessionState } from "../state.ts"
import { syncSession } from "../sync.ts"
import type { RuntimeContext, SessionState, StartReviewSyncInput } from "../types.ts"
import { isAncestor, resolveMergeBase } from "./history.ts"
import { resumeSession } from "./resume.ts"

/** Creates or reuses one durable review-sync session and runs the first refresh. */
export async function startReviewSync(input: StartReviewSyncInput) {
  const context = createRuntimeContext(input.cwd)
  const { result } = await startReviewSyncWithSession(input.agentBranch, context)
  return result
}

/** Performs the start workflow and keeps the loaded session for command composition. */
export async function startReviewSyncWithSession(agentBranch: string, context: RuntimeContext) {
  const session = await createSessionForStart(agentBranch, context)
  return await startLoadedReviewSyncSession(session, context)
}

/** Runs the start refresh for an already resolved session, reactivating it if needed. */
export async function startLoadedReviewSyncSession(session: SessionState, context: RuntimeContext) {
  await seedRenderedSnapshotForExistingReviewBranch(session, context)
  await prepareReviewBranchForStart(session, context)
  await seedRenderedSnapshotForExistingReviewBranch(session, context)
  const active = await resumeSession(session, "start")
  const syncResult = await syncSession(active, context)
  const latest = await readSessionState(active)

  const result = createReviewSyncResult({
    exitCode: 0,
    command: "start",
    status: syncResult.status === "rejected-human-patch" ? "rejected-human-patch" : "ok",
    sessionId: latest.sessionId,
    reviewBranch: latest.reviewBranch,
    acceptedPatchPath: syncResult.acceptedPatchPath ?? undefined,
    rejectedPatchPath: syncResult.rejectedPatchPath ?? undefined,
    message:
      syncResult.status === "rejected-human-patch"
        ? `Started review sync for ${latest.agentBranch} as ${latest.reviewBranch}; human patch was rejected and saved to ${syncResult.rejectedPatchPath}.`
        : `Started review sync for ${latest.agentBranch} as ${latest.reviewBranch}.`,
  })
  return { session: latest, result }
}

/** Seeds the first rendered baseline when a review branch already carries human work. */
async function seedRenderedSnapshotForExistingReviewBranch(
  session: SessionState,
  context: RuntimeContext,
) {
  const renderedSnapshot = await resolveRef(
    session.agentWorktree,
    session.refs.renderedSnapshot,
    context,
  )
  if (renderedSnapshot) {
    return
  }

  const currentBranch = await resolveCurrentBranch(session.reviewWorktree, context)
  if (currentBranch !== session.reviewBranch) {
    return
  }
  const currentHead = await resolveRef(session.reviewWorktree, "HEAD", context)
  if (!currentHead) {
    return
  }

  const agentHead = await resolveRef(
    session.agentWorktree,
    `refs/heads/${session.agentBranch}`,
    context,
  )
  if (!agentHead) {
    throw new UserError(`Agent branch ${session.agentBranch} no longer exists.`)
  }

  const baseline = await resolveExistingReviewBranchBaseline({
    cwd: session.reviewWorktree,
    currentHead,
    agentHead,
    clean: await isWorktreeClean(session.reviewWorktree, context),
    context,
  })
  if (!baseline) {
    return
  }

  await updateRef(session.agentWorktree, session.refs.renderedSnapshot, baseline, context)
}

/** Infers the rendered baseline for human work made before a session existed. */
async function resolveExistingReviewBranchBaseline(input: {
  cwd: string
  currentHead: string
  agentHead: string
  clean: boolean
  context: RuntimeContext
}) {
  if (input.currentHead === input.agentHead) {
    return input.clean ? null : input.currentHead
  }

  if (await isAncestor(input.cwd, input.agentHead, input.currentHead, input.context)) {
    return input.agentHead
  }
  if (await isAncestor(input.cwd, input.currentHead, input.agentHead, input.context)) {
    return input.clean ? null : input.currentHead
  }

  return await resolveMergeBase(input.cwd, input.agentHead, input.currentHead, input.context)
}

/** Selects an eligible checked-out agent branch when start runs interactively. */
async function promptForAgentBranch(context: RuntimeContext) {
  const choices = await listAgentWorktreeChoicesForStart(context)
  if (choices.length === 0) {
    throw new UserError("No eligible agent worktrees are checked out for this repository.")
  }
  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    throw new UserError("start requires an agent branch when running non-interactively.")
  }

  const selected = await autocomplete({
    message: "Pick an agent branch",
    placeholder: "Type to filter branches...",
    options: choices.map((choice) => ({
      value: choice.branch,
      label: choice.branch,
      hint: choice.path,
    })),
  })
  if (isCancel(selected)) {
    cancel("Canceled.")
    throw new UserError("Start canceled.", "error", 130)
  }
  return selected
}

/** Builds the start subcommand. */
export function createStartCommand(cwd: string) {
  return command({
    name: "start",
    description: "Create or reuse a review branch and run the initial sync",
    args: {
      agentBranch: positional({
        type: optional(string),
        displayName: "agent-branch",
        description: "Agent branch checked out in another worktree",
      }),
    },
    handler: async ({ agentBranch }) => {
      const context = createRuntimeContext(cwd)
      const resolvedAgentBranch = agentBranch ?? (await promptForAgentBranch(context))
      return await startReviewSync({ cwd, agentBranch: resolvedAgentBranch })
    },
  })
}

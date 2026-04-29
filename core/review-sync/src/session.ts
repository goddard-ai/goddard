/** Session start, inference, and validation helpers. */
import { join, resolve } from "node:path"

import { UserError } from "./errors.ts"
import {
  assertReviewBranchNotCheckedOutElsewhere,
  assertSupportedGitState,
  branchExists,
  git,
  isInsideOrEqual,
  isWorktreeClean,
  normalizePath,
  pathExists,
  resolveCurrentBranch,
  resolveRef,
  resolveRequiredGitCommonDir,
  resolveRequiredRepoRoot,
} from "./git.ts"
import {
  appendEvent,
  createSessionId,
  ensureSessionDirs,
  findSessionByReviewBranch,
  listSessions,
  readSessionStateFile,
  resolveSessionDir,
  writeSessionState,
} from "./state.ts"
import {
  reviewBranchSuffix,
  schemaVersion,
  type RuntimeContext,
  type SessionState,
} from "./types.ts"

/** Resolves, validates, and creates or loads one start-command session. */
export async function createSessionForStart(reviewWorktreeInput: string, context: RuntimeContext) {
  const agentWorktree = await resolveRequiredRepoRoot(context.cwd, context)
  await assertSupportedGitState(agentWorktree, context)

  const agentBranch = await resolveCurrentBranch(agentWorktree, context)
  if (!agentBranch) {
    throw new UserError("start must run from an attached agent branch, not detached HEAD.")
  }

  if (agentBranch.endsWith(reviewBranchSuffix)) {
    throw new UserError(`Agent branch ${agentBranch} already ends with ${reviewBranchSuffix}.`)
  }

  const reviewWorktree = await resolveRequiredRepoRoot(
    resolve(context.cwd, reviewWorktreeInput),
    context,
  )
  await assertSupportedGitState(reviewWorktree, context)

  const [agentCommonDir, reviewCommonDir] = await Promise.all([
    resolveRequiredGitCommonDir(agentWorktree, context),
    resolveRequiredGitCommonDir(reviewWorktree, context),
  ])
  if (agentCommonDir !== reviewCommonDir) {
    throw new UserError("Agent and review worktrees must belong to the same Git repository.")
  }

  const reviewBranch = `${agentBranch}${reviewBranchSuffix}`
  await assertReviewBranchNotCheckedOutElsewhere({
    cwd: agentWorktree,
    reviewBranch,
    reviewWorktree,
    context,
  })

  return await loadOrCreateSession({
    sessionId: createSessionId({
      repoCommonDir: agentCommonDir,
      agentWorktree,
      reviewWorktree,
      agentBranch,
    }),
    repoCommonDir: agentCommonDir,
    agentWorktree,
    reviewWorktree,
    agentBranch,
    reviewBranch,
  })
}

/** Ensures the review branch exists and is checked out before any session sync can run. */
export async function prepareReviewBranchForStart(session: SessionState, context: RuntimeContext) {
  const currentBranch = await resolveCurrentBranch(session.reviewWorktree, context)
  if (currentBranch === session.reviewBranch) {
    const renderedSnapshot = await resolveRef(
      session.agentWorktree,
      session.refs.renderedSnapshot,
      context,
    )
    if (!renderedSnapshot && !(await isWorktreeClean(session.reviewWorktree, context))) {
      throw new UserError(
        `Review worktree ${session.reviewWorktree} has local changes without a rendered baseline; clean it before start refreshes ${session.reviewBranch}.`,
      )
    }
    return
  }

  if (!(await isWorktreeClean(session.reviewWorktree, context))) {
    throw new UserError(
      `Review worktree ${session.reviewWorktree} has local changes; clean it before start checks out ${session.reviewBranch}.`,
    )
  }

  if (await branchExists(session.reviewWorktree, session.reviewBranch, context)) {
    await git(session.reviewWorktree, ["checkout", session.reviewBranch], context, {
      stdin: "ignore",
    })
    return
  }

  const agentHead = (await git(session.agentWorktree, ["rev-parse", "HEAD"], context)).stdout.trim()
  await git(session.reviewWorktree, ["branch", session.reviewBranch, agentHead], context)
  await git(session.reviewWorktree, ["checkout", session.reviewBranch], context, {
    stdin: "ignore",
  })
}

/** Infers the review-sync session from the current worktree path or checked-out branch. */
export async function inferSession(context: RuntimeContext) {
  const repoRoot = await resolveRequiredRepoRoot(context.cwd, context)
  const commonDir = await resolveRequiredGitCommonDir(repoRoot, context)
  const sessions = await listSessions(commonDir)
  const cwd = await normalizePath(context.cwd)
  const pathMatches = sessions.filter(
    (session) =>
      isInsideOrEqual(session.agentWorktree, cwd) || isInsideOrEqual(session.reviewWorktree, cwd),
  )

  if (pathMatches.length === 1) {
    return pathMatches[0]!
  }
  if (pathMatches.length > 1) {
    throw new UserError(`Multiple review-sync sessions match ${cwd}.`)
  }

  const branch = await resolveCurrentBranch(repoRoot, context)
  const branchMatches = sessions.filter(
    (session) => branch === session.agentBranch || branch === session.reviewBranch,
  )
  if (branchMatches.length === 1) {
    return branchMatches[0]!
  }

  throw new UserError("No review-sync session matches the current worktree.")
}

/** Validates branch identity and unsupported Git operation states before a sync mutation. */
export async function validateSessionWorktrees(session: SessionState, context: RuntimeContext) {
  await assertSupportedGitState(session.agentWorktree, context)
  await assertSupportedGitState(session.reviewWorktree, context)

  const agentBranch = await resolveCurrentBranch(session.agentWorktree, context)
  if (agentBranch !== session.agentBranch) {
    throw new UserError(
      `Agent worktree must be on ${session.agentBranch}; currently ${agentBranch ?? "detached HEAD"}.`,
    )
  }

  const reviewBranch = await resolveCurrentBranch(session.reviewWorktree, context)
  if (reviewBranch !== session.reviewBranch) {
    throw new UserError(
      `Review worktree must be on ${session.reviewBranch}; currently ${reviewBranch ?? "detached HEAD"}.`,
    )
  }

  const [agentCommonDir, reviewCommonDir] = await Promise.all([
    resolveRequiredGitCommonDir(session.agentWorktree, context),
    resolveRequiredGitCommonDir(session.reviewWorktree, context),
  ])
  if (agentCommonDir !== session.repoCommonDir || reviewCommonDir !== session.repoCommonDir) {
    throw new UserError(
      "Review sync session worktrees no longer share the recorded Git common dir.",
    )
  }
}

/** Loads an existing session or writes a new state file for this branch/worktree pair. */
async function loadOrCreateSession(input: {
  sessionId: string
  repoCommonDir: string
  agentWorktree: string
  reviewWorktree: string
  agentBranch: string
  reviewBranch: string
}) {
  const existing = await findSessionByReviewBranch(input.repoCommonDir, input.reviewBranch)
  if (existing && existing.sessionId !== input.sessionId) {
    throw new UserError(
      `Review branch ${input.reviewBranch} is already owned by session ${existing.sessionId}.`,
    )
  }

  const statePath = join(resolveSessionDir(input.repoCommonDir, input.sessionId), "state.json")
  if (await pathExists(statePath)) {
    return await readSessionStateFile(statePath)
  }

  const now = new Date().toISOString()
  const state = {
    schemaVersion,
    sessionId: input.sessionId,
    repoCommonDir: input.repoCommonDir,
    agentWorktree: input.agentWorktree,
    reviewWorktree: input.reviewWorktree,
    agentBranch: input.agentBranch,
    reviewBranch: input.reviewBranch,
    refs: {
      agentSnapshot: `refs/review-sync/${input.sessionId}/agent-snapshot`,
      renderedSnapshot: `refs/review-sync/${input.sessionId}/rendered-snapshot`,
    },
    paused: false,
    createdAt: now,
    updatedAt: now,
    lastSync: {
      status: "synced",
      acceptedPatch: null,
      rejectedPatch: null,
    },
  } satisfies SessionState
  await ensureSessionDirs(state)
  await writeSessionState(state)
  await appendEvent(state, {
    command: "start",
    status: "ok",
    reviewBranch: state.reviewBranch,
  })
  return state
}

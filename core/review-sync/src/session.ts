/** Session start, inference, and validation helpers. */
import { join } from "node:path"

import { UserError } from "./errors.ts"
import {
  assertReviewBranchNotCheckedOutElsewhere,
  assertSupportedGitState,
  branchExists,
  git,
  isInsideOrEqual,
  isWorktreeClean,
  listGitWorktrees,
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
  reviewBranchPrefix,
  schemaVersion,
  type RuntimeContext,
  type SessionState,
} from "./types.ts"

/** Candidate agent branch already checked out in a sibling worktree. */
export type AgentWorktreeChoice = {
  branch: string
  path: string
}

/** Signals that the requested agent branch is not currently owned by a worktree. */
export class AgentBranchWorktreeMissingError extends UserError {
  readonly agentBranch

  constructor(agentBranch: string) {
    super(`Agent branch ${agentBranch} is not checked out in another worktree.`)
    this.agentBranch = agentBranch
  }
}

/** Signals a temporary agent checkout mismatch that callers can wait through. */
export class AgentWorktreeCheckoutMismatchError extends UserError {
  readonly worktree
  readonly expectedBranch
  readonly actualBranch

  constructor(input: { worktree: string; expectedBranch: string; actualBranch: string | null }) {
    super(
      `Agent worktree ${input.worktree} must be on ${input.expectedBranch}; currently ${input.actualBranch ?? "detached HEAD"}.`,
    )
    this.worktree = input.worktree
    this.expectedBranch = input.expectedBranch
    this.actualBranch = input.actualBranch
  }
}

/** Resolves, validates, and creates or loads one start-command session. */
export async function createSessionForStart(agentBranchInput: string, context: RuntimeContext) {
  const agentBranch = agentBranchInput.trim()
  if (!agentBranch) {
    throw new UserError("start requires an agent branch.")
  }

  const reviewWorktree = await resolveRequiredRepoRoot(context.cwd, context)
  await assertSupportedGitState(reviewWorktree, context)

  if (agentBranch.startsWith(reviewBranchPrefix)) {
    throw new UserError(`Agent branch ${agentBranch} already starts with ${reviewBranchPrefix}.`)
  }

  const agentWorktree = await resolveAgentWorktreeForStart(agentBranch, reviewWorktree, context)
  await assertSupportedGitState(agentWorktree, context)

  const [agentCommonDir, reviewCommonDir] = await Promise.all([
    resolveRequiredGitCommonDir(agentWorktree, context),
    resolveRequiredGitCommonDir(reviewWorktree, context),
  ])
  if (agentCommonDir !== reviewCommonDir) {
    throw new UserError("Agent and review worktrees must belong to the same Git repository.")
  }

  const reviewBranch = `${reviewBranchPrefix}${agentBranch}`
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

/** Lists branches that can be used as agent worktrees for start from the current cwd. */
export async function listAgentWorktreeChoicesForStart(context: RuntimeContext) {
  const reviewWorktree = await resolveRequiredRepoRoot(context.cwd, context)
  return await listAgentWorktreeChoices(reviewWorktree, context)
}

/** Lists eligible agent worktrees outside the review worktree. */
async function listAgentWorktreeChoices(reviewWorktree: string, context: RuntimeContext) {
  const worktrees = await listGitWorktrees(reviewWorktree, context)
  const choices: AgentWorktreeChoice[] = []

  for (const worktree of worktrees) {
    if (
      !worktree.branch ||
      worktree.path === reviewWorktree ||
      worktree.branch.startsWith(reviewBranchPrefix)
    ) {
      continue
    }
    choices.push({
      branch: worktree.branch,
      path: worktree.path,
    })
  }

  return choices.sort((left, right) => left.branch.localeCompare(right.branch))
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

/** Resolves the checked-out worktree that owns the requested agent branch. */
async function resolveAgentWorktreeForStart(
  agentBranch: string,
  reviewWorktree: string,
  context: RuntimeContext,
) {
  const choices = await listAgentWorktreeChoices(reviewWorktree, context)
  const match = choices.find((choice) => choice.branch === agentBranch)
  if (!match) {
    throw new AgentBranchWorktreeMissingError(agentBranch)
  }
  return match.path
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
    throw new AgentWorktreeCheckoutMismatchError({
      worktree: session.agentWorktree,
      expectedBranch: session.agentBranch,
      actualBranch: agentBranch,
    })
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

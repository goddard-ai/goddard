/** Public entrypoint for Git-backed agent review branch synchronization. */
import { spawn } from "node:child_process"
import { createHash } from "node:crypto"
import { constants as fsConstants } from "node:fs"
import {
  access,
  appendFile,
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  realpath,
  rename,
  rm,
  writeFile,
} from "node:fs/promises"
import { hostname, tmpdir } from "node:os"
import { basename, isAbsolute, join, relative, resolve } from "node:path"

type ReviewSyncCommand = "start" | "sync" | "status" | "pause" | "resume"

type ReviewSyncStatus = "ok" | "rejected-human-patch" | "paused" | "error"

type LastSyncStatus = "synced" | "rejected-human-patch" | "paused" | "error"

type ParsedCommand =
  | { command: "start"; reviewWorktree: string }
  | { command: "sync" }
  | { command: "status"; json: boolean }
  | { command: "pause" }
  | { command: "resume" }

type WritableStreamLike = {
  write(chunk: string | Uint8Array): unknown
}

/** Runtime hooks accepted by the CLI-compatible programmatic entrypoint. */
export type ReviewSyncEnv = {
  cwd?: string
  stdout?: WritableStreamLike
  stderr?: WritableStreamLike
  env?: Record<string, string | undefined>
}

/** Structured command result returned by both CLI and embedded callers. */
export type ReviewSyncResult = {
  exitCode: number
  command: ReviewSyncCommand
  status: ReviewSyncStatus
  sessionId?: string
  reviewBranch?: string
  acceptedPatchPath?: string
  rejectedPatchPath?: string
  message: string
}

type RuntimeContext = {
  cwd: string
  stdout?: WritableStreamLike
  stderr?: WritableStreamLike
  env: Record<string, string | undefined>
}

type SessionState = {
  schemaVersion: 1
  sessionId: string
  repoCommonDir: string
  agentWorktree: string
  reviewWorktree: string
  agentBranch: string
  reviewBranch: string
  refs: {
    agentSnapshot: string
    renderedSnapshot: string
  }
  paused: boolean
  createdAt: string
  updatedAt: string
  lastSync: {
    status: LastSyncStatus
    acceptedPatch: string | null
    rejectedPatch: string | null
  }
}

type WorktreeInfo = {
  path: string
  branch: string | null
}

type CommandResult = {
  status: number
  stdout: string
  stderr: string
}

type PatchFlowResult = {
  status: "synced" | "rejected-human-patch"
  acceptedPatchPath: string | null
  rejectedPatchPath: string | null
}

const schemaVersion = 1
const reviewBranchSuffix = "--review"
const lockStaleAfterMs = 10 * 60 * 1000

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

  const sessionId = createSessionId({
    repoCommonDir: agentCommonDir,
    agentWorktree,
    reviewWorktree,
    agentBranch,
  })
  const session = await loadOrCreateSession({
    sessionId,
    repoCommonDir: agentCommonDir,
    agentWorktree,
    reviewWorktree,
    agentBranch,
    reviewBranch,
    context,
  })

  await prepareReviewBranchForStart(session, context)
  const syncResult = await syncSession(session, context)

  return createReviewSyncResult({
    exitCode: syncResult.status === "rejected-human-patch" ? 0 : 0,
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

/** Coordinates one complete patch-acceptance and review-refresh cycle. */
async function syncSession(session: SessionState, context: RuntimeContext) {
  return await withSessionLock(session, context, async () => {
    const latest = await readSessionState(session)
    if (latest.paused) {
      await appendEvent(latest, {
        command: "sync",
        status: "paused",
      })
      throw new UserError(`Review sync session ${latest.sessionId} is paused.`, "paused", 0)
    }

    await validateSessionWorktrees(latest, context)
    const patchResult = await handleHumanPatch(latest, context)
    const agentSnapshot = await createSnapshotCommit({
      cwd: latest.agentWorktree,
      label: `${latest.sessionId}:agent`,
      context,
    })

    await updateRef(latest.agentWorktree, latest.refs.agentSnapshot, agentSnapshot, context)
    await refreshReviewWorktree(latest, agentSnapshot, context)
    await updateRef(latest.agentWorktree, latest.refs.renderedSnapshot, agentSnapshot, context)

    latest.updatedAt = new Date().toISOString()
    latest.lastSync = {
      status: patchResult.status,
      acceptedPatch: patchResult.acceptedPatchPath,
      rejectedPatch: patchResult.rejectedPatchPath,
    }
    await writeSessionState(latest)
    await appendEvent(latest, {
      command: "sync",
      status: patchResult.status,
      acceptedPatchPath: patchResult.acceptedPatchPath,
      rejectedPatchPath: patchResult.rejectedPatchPath,
    })
    return patchResult
  })
}

/** Computes and applies the human patch when a rendered baseline already exists. */
async function handleHumanPatch(session: SessionState, context: RuntimeContext) {
  const renderedSnapshot = await resolveRef(
    session.agentWorktree,
    session.refs.renderedSnapshot,
    context,
  )

  if (!renderedSnapshot) {
    return createPatchFlowResult({
      status: "synced",
      acceptedPatchPath: null,
      rejectedPatchPath: null,
    })
  }

  const reviewSnapshot = await createSnapshotCommit({
    cwd: session.reviewWorktree,
    label: `${session.sessionId}:review`,
    context,
  })
  const patch = await diffCommits(session.agentWorktree, renderedSnapshot, reviewSnapshot, context)

  if (!patch.trim()) {
    return createPatchFlowResult({
      status: "synced",
      acceptedPatchPath: null,
      rejectedPatchPath: null,
    })
  }

  const check = await git(session.agentWorktree, ["apply", "--check", "--binary"], context, {
    allowFailure: true,
    stdin: patch,
  })
  if (check.status !== 0) {
    const rejectedPatchPath = await savePatch(session, "rejected", patch)
    return createPatchFlowResult({
      status: "rejected-human-patch",
      acceptedPatchPath: null,
      rejectedPatchPath,
    })
  }

  const acceptedPatchPath = await savePatch(session, "accepted", patch)
  const apply = await git(session.agentWorktree, ["apply", "--binary"], context, {
    allowFailure: true,
    stdin: patch,
  })
  if (apply.status !== 0) {
    throw new UserError(
      `Human patch passed preflight but failed during apply; recovery is required in ${session.agentWorktree}.`,
    )
  }

  return createPatchFlowResult({
    status: "synced",
    acceptedPatchPath,
    rejectedPatchPath: null,
  })
}

/** Preserves the narrow patch-flow status type across async control-flow branches. */
function createPatchFlowResult(result: PatchFlowResult) {
  return result
}

/** Captures tracked, modified, deleted, and untracked non-ignored files into one commit. */
async function createSnapshotCommit(input: {
  cwd: string
  label: string
  context: RuntimeContext
}) {
  const scratchDir = await mkdtemp(join(tmpdir(), `review-sync-${process.pid}-`))
  const indexPath = join(
    scratchDir,
    `${createHash("sha256").update(input.cwd).digest("hex")}.index`,
  )

  try {
    await git(input.cwd, ["read-tree", "HEAD"], input.context, {
      env: { GIT_INDEX_FILE: indexPath },
    })
    await git(input.cwd, ["add", "-A", "--", "."], input.context, {
      env: { GIT_INDEX_FILE: indexPath },
    })
    const tree = (
      await git(input.cwd, ["write-tree"], input.context, {
        env: { GIT_INDEX_FILE: indexPath },
      })
    ).stdout.trim()
    const commit = await git(
      input.cwd,
      ["commit-tree", tree, "-p", "HEAD", "-m", `review-sync:${input.label}`],
      input.context,
      {
        env: {
          GIT_INDEX_FILE: indexPath,
          GIT_AUTHOR_NAME: "Review Sync",
          GIT_AUTHOR_EMAIL: "review-sync@local",
          GIT_COMMITTER_NAME: "Review Sync",
          GIT_COMMITTER_EMAIL: "review-sync@local",
        },
      },
    )
    return commit.stdout.trim()
  } finally {
    await rm(scratchDir, { recursive: true, force: true }).catch(() => {})
  }
}

/** Moves the checked-out review branch to the latest agent snapshot and cleans mirror state. */
async function refreshReviewWorktree(
  session: SessionState,
  agentSnapshot: string,
  context: RuntimeContext,
) {
  await git(session.reviewWorktree, ["reset", "--hard", agentSnapshot], context)
  await git(session.reviewWorktree, ["clean", "-fd"], context)
}

/** Ensures the review branch exists and is checked out before any session sync can run. */
async function prepareReviewBranchForStart(session: SessionState, context: RuntimeContext) {
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

/** Validates branch identity and unsupported Git operation states before a sync mutation. */
async function validateSessionWorktrees(session: SessionState, context: RuntimeContext) {
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
  context: RuntimeContext
}) {
  const existing = await findSessionByReviewBranch(input.repoCommonDir, input.reviewBranch)
  if (existing && existing.sessionId !== input.sessionId) {
    throw new UserError(
      `Review branch ${input.reviewBranch} is already owned by session ${existing.sessionId}.`,
    )
  }

  const sessionDir = resolveSessionDir(input.repoCommonDir, input.sessionId)
  const statePath = join(sessionDir, "state.json")
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

/** Infers the review-sync session from the current worktree path or checked-out branch. */
async function inferSession(context: RuntimeContext) {
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

/** Writes accepted or rejected patch contents to a deterministic fingerprinted file. */
async function savePatch(session: SessionState, kind: "accepted" | "rejected", patch: string) {
  const patchDir = join(
    resolveSessionDir(session.repoCommonDir, session.sessionId),
    "patches",
    kind,
  )
  await mkdir(patchDir, { recursive: true })
  const patchPath = join(patchDir, `${createHash("sha256").update(patch).digest("hex")}.patch`)
  try {
    await writeFile(patchPath, patch, { flag: "wx" })
  } catch (error) {
    if (!isNodeErrorWithCode(error, "EEXIST")) {
      throw error
    }
  }
  return patchPath
}

/** Appends one audit event as newline-delimited JSON. */
async function appendEvent(
  session: SessionState,
  event: Record<string, string | number | boolean | null>,
) {
  const sessionDir = resolveSessionDir(session.repoCommonDir, session.sessionId)
  await mkdir(sessionDir, { recursive: true })
  await appendFile(
    join(sessionDir, "events.ndjson"),
    `${JSON.stringify({
      timestamp: new Date().toISOString(),
      sessionId: session.sessionId,
      agentBranch: session.agentBranch,
      reviewBranch: session.reviewBranch,
      ...event,
    })}\n`,
  )
}

/** Acquires the session lock for one mutating operation and releases it on completion. */
async function withSessionLock<T>(
  session: SessionState,
  _context: RuntimeContext,
  work: () => Promise<T>,
) {
  const lockDir = join(resolveSessionDir(session.repoCommonDir, session.sessionId), "lock")
  const release = await acquireLock(lockDir)
  try {
    return await work()
  } finally {
    await release()
  }
}

/** Creates an atomic lock directory, reclaiming stale local-process locks. */
async function acquireLock(lockDir: string) {
  await mkdir(resolve(lockDir, ".."), { recursive: true })
  try {
    await mkdir(lockDir)
  } catch (error) {
    if (!isNodeErrorWithCode(error, "EEXIST")) {
      throw error
    }
    if (!(await isLockStale(lockDir))) {
      throw new UserError(`Review sync session is already locked: ${lockDir}`)
    }
    await rm(lockDir, { recursive: true, force: true })
    await mkdir(lockDir)
  }

  await writeFile(
    join(lockDir, "owner.json"),
    JSON.stringify({
      pid: process.pid,
      hostname: hostname(),
      acquiredAt: Date.now(),
    }),
  )

  return async () => {
    await rm(lockDir, { recursive: true, force: true }).catch(() => {})
  }
}

/** Treats locks from dead same-host processes, malformed locks, or old locks as stale. */
async function isLockStale(lockDir: string) {
  try {
    const owner = JSON.parse(await readFile(join(lockDir, "owner.json"), "utf-8")) as {
      pid?: number
      hostname?: string
      acquiredAt?: number
    }
    if (typeof owner.acquiredAt === "number" && Date.now() - owner.acquiredAt > lockStaleAfterMs) {
      return true
    }
    if (owner.hostname && owner.hostname !== hostname()) {
      return false
    }
    if (typeof owner.pid === "number") {
      return !isProcessAlive(owner.pid)
    }
    return true
  } catch {
    return true
  }
}

/** Finds a session that already owns one review branch. */
async function findSessionByReviewBranch(commonDir: string, reviewBranch: string) {
  const sessions = await listSessions(commonDir)
  return sessions.find((session) => session.reviewBranch === reviewBranch) ?? null
}

/** Reads every valid session state file for one repository common directory. */
async function listSessions(commonDir: string) {
  const sessionsRoot = resolveSessionsRoot(commonDir)
  let entries: string[]
  try {
    entries = await readdir(sessionsRoot)
  } catch {
    return []
  }

  const sessions: SessionState[] = []
  for (const entry of entries) {
    try {
      sessions.push(await readSessionStateFile(join(sessionsRoot, entry, "state.json")))
    } catch {
      // Ignore malformed or partially written session directories during discovery.
    }
  }
  return sessions
}

/** Reads a known session state file and validates the schema version. */
async function readSessionState(session: SessionState) {
  return await readSessionStateFile(
    join(resolveSessionDir(session.repoCommonDir, session.sessionId), "state.json"),
  )
}

/** Parses a durable state JSON file. */
async function readSessionStateFile(statePath: string) {
  const parsed = JSON.parse(await readFile(statePath, "utf-8")) as SessionState
  if (parsed.schemaVersion !== schemaVersion) {
    throw new Error(`Unsupported review-sync state schema in ${statePath}.`)
  }
  return parsed
}

/** Writes state through a temporary file before renaming it over the durable state path. */
async function writeSessionState(session: SessionState) {
  await ensureSessionDirs(session)
  const statePath = join(resolveSessionDir(session.repoCommonDir, session.sessionId), "state.json")
  const tmpPath = `${statePath}.${process.pid}.${Date.now()}.tmp`
  await writeFile(tmpPath, `${JSON.stringify(session, null, 2)}\n`)
  await rename(tmpPath, statePath)
}

/** Creates the session directory tree used for state, locks, events, and patches. */
async function ensureSessionDirs(session: SessionState) {
  const sessionDir = resolveSessionDir(session.repoCommonDir, session.sessionId)
  await mkdir(join(sessionDir, "patches", "accepted"), { recursive: true })
  await mkdir(join(sessionDir, "patches", "rejected"), { recursive: true })
  await mkdir(join(sessionDir, "patches", "pending"), { recursive: true })
}

/** Ensures a review branch is not checked out outside the configured review worktree. */
async function assertReviewBranchNotCheckedOutElsewhere(input: {
  cwd: string
  reviewBranch: string
  reviewWorktree: string
  context: RuntimeContext
}) {
  const worktrees = await listGitWorktrees(input.cwd, input.context)
  for (const worktree of worktrees) {
    if (worktree.branch !== input.reviewBranch) {
      continue
    }
    if (worktree.path !== input.reviewWorktree) {
      throw new UserError(
        `Review branch ${input.reviewBranch} is already checked out at ${worktree.path}.`,
      )
    }
  }
}

/** Parses Git's porcelain worktree list into path and branch records. */
async function listGitWorktrees(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["worktree", "list", "--porcelain"], context)
  const worktrees: WorktreeInfo[] = []
  let current: Partial<WorktreeInfo> = {}

  for (const line of result.stdout.split("\n")) {
    if (!line.trim()) {
      if (current.path) {
        worktrees.push({
          path: await normalizePath(current.path),
          branch: current.branch ?? null,
        })
      }
      current = {}
      continue
    }

    if (line.startsWith("worktree ")) {
      current.path = line.slice("worktree ".length)
    } else if (line.startsWith("branch refs/heads/")) {
      current.branch = line.slice("branch refs/heads/".length)
    }
  }

  if (current.path) {
    worktrees.push({
      path: await normalizePath(current.path),
      branch: current.branch ?? null,
    })
  }

  return worktrees
}

/** Refuses sync while Git has an unresolved operation in progress. */
async function assertSupportedGitState(cwd: string, context: RuntimeContext) {
  const gitDir = await resolveRequiredGitDir(cwd, context)
  const commonDir = await resolveRequiredGitCommonDir(cwd, context)
  const markers = [
    join(gitDir, "MERGE_HEAD"),
    join(gitDir, "CHERRY_PICK_HEAD"),
    join(gitDir, "REVERT_HEAD"),
    join(gitDir, "REBASE_HEAD"),
    join(gitDir, "rebase-merge"),
    join(gitDir, "rebase-apply"),
    join(gitDir, "BISECT_LOG"),
    join(commonDir, "BISECT_LOG"),
  ]

  for (const marker of markers) {
    if (await pathExists(marker)) {
      throw new UserError(`Unsupported in-progress Git state in ${cwd}: ${basename(marker)}.`)
    }
  }
}

/** Resolves the repository root for a path or raises a user-facing error. */
async function resolveRequiredRepoRoot(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--show-toplevel"], context, {
    allowFailure: true,
  })
  if (result.status !== 0 || !result.stdout.trim()) {
    throw new UserError(`Not a Git worktree: ${cwd}`)
  }
  return await normalizePath(result.stdout.trim())
}

/** Resolves one worktree's Git common directory as an absolute path. */
async function resolveRequiredGitCommonDir(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--git-common-dir"], context)
  const value = result.stdout.trim()
  return await normalizePath(isAbsolute(value) ? value : resolve(cwd, value))
}

/** Resolves one worktree's per-worktree Git metadata directory as an absolute path. */
async function resolveRequiredGitDir(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--git-dir"], context)
  const value = result.stdout.trim()
  return await normalizePath(isAbsolute(value) ? value : resolve(cwd, value))
}

/** Returns the attached branch name, or null for detached HEAD. */
async function resolveCurrentBranch(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"], context, {
    allowFailure: true,
  })
  return result.status === 0 ? result.stdout.trim() || null : null
}

/** Checks whether a local branch already exists. */
async function branchExists(cwd: string, branch: string, context: RuntimeContext) {
  const result = await git(
    cwd,
    ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`],
    context,
    {
      allowFailure: true,
    },
  )
  return result.status === 0
}

/** Checks whether a worktree has tracked, unstaged, staged, or untracked non-ignored changes. */
async function isWorktreeClean(cwd: string, context: RuntimeContext) {
  const result = await git(cwd, ["status", "--porcelain=v1", "--untracked-files=all"], context)
  return !result.stdout.trim()
}

/** Reads one ref and returns null when it does not exist. */
async function resolveRef(cwd: string, refName: string, context: RuntimeContext) {
  const result = await git(cwd, ["rev-parse", "--verify", "-q", refName], context, {
    allowFailure: true,
  })
  return result.status === 0 ? result.stdout.trim() || null : null
}

/** Updates or creates one hidden ref. */
async function updateRef(cwd: string, refName: string, oid: string, context: RuntimeContext) {
  await git(cwd, ["update-ref", refName, oid], context)
}

/** Computes a binary Git diff between two snapshot commits. */
async function diffCommits(cwd: string, before: string, after: string, context: RuntimeContext) {
  return (await git(cwd, ["diff", "--binary", before, after], context)).stdout
}

/** Counts fingerprinted patch files in one patch category directory. */
async function countPatchFiles(patchDir: string) {
  try {
    const entries = await readdir(patchDir)
    return entries.filter((entry) => entry.endsWith(".patch")).length
  } catch {
    return 0
  }
}

/** Creates a deterministic, ref-safe session id for one agent/review pairing. */
function createSessionId(input: {
  repoCommonDir: string
  agentWorktree: string
  reviewWorktree: string
  agentBranch: string
}) {
  return `sha256-${createHash("sha256")
    .update(
      JSON.stringify({
        repoCommonDir: input.repoCommonDir,
        agentWorktree: input.agentWorktree,
        reviewWorktree: input.reviewWorktree,
        agentBranch: input.agentBranch,
      }),
    )
    .digest("hex")}`
}

/** Returns the directory containing every session for one repository common dir. */
function resolveSessionsRoot(commonDir: string) {
  return join(commonDir, "review-sync", "sessions")
}

/** Returns the durable state directory for one session. */
function resolveSessionDir(commonDir: string, sessionId: string) {
  return join(resolveSessionsRoot(commonDir), sessionId)
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

/** Normalizes optional runtime hooks to concrete values. */
function createRuntimeContext(env: ReviewSyncEnv) {
  return {
    cwd: env.cwd ?? process.cwd(),
    stdout: env.stdout,
    stderr: env.stderr,
    env: env.env ?? process.env,
  } satisfies RuntimeContext
}

/** Writes command output to stdout for successful states and stderr for hard errors. */
function writeResult(context: RuntimeContext, result: ReviewSyncResult) {
  const target = result.status === "error" ? context.stderr : context.stdout
  target?.write(`${result.message}\n`)
}

/** Converts thrown errors into the stable command result contract. */
function createErrorResult(command: ReviewSyncCommand, error: unknown) {
  const userError = error instanceof UserError ? error : null
  return createReviewSyncResult({
    exitCode: userError?.exitCode ?? 1,
    command,
    status: userError?.status ?? "error",
    message: error instanceof Error ? error.message : String(error),
  })
}

/** Preserves the public result shape for callers even when fields are absent at runtime. */
function createReviewSyncResult(result: ReviewSyncResult) {
  return result
}

/** Runs one Git command and returns captured stdout/stderr. */
async function git(
  cwd: string,
  args: string[],
  context: RuntimeContext,
  options: {
    allowFailure?: boolean
    stdin?: string | "ignore"
    env?: Record<string, string | undefined>
  } = {},
) {
  const result = await runCommand("git", args, {
    cwd,
    stdin: options.stdin,
    env: {
      ...context.env,
      ...options.env,
    },
  })

  if (result.status !== 0 && options.allowFailure !== true) {
    throw new Error(
      `git ${args.join(" ")} failed in ${cwd}: ${
        result.stderr.trim() || result.stdout.trim() || "unknown Git error"
      }`,
    )
  }

  return result
}

/** Runs a subprocess with captured output and optional stdin. */
async function runCommand(
  command: string,
  args: string[],
  options: {
    cwd: string
    stdin?: string | "ignore"
    env: Record<string, string | undefined>
  },
) {
  return await new Promise<CommandResult>((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["pipe", "pipe", "pipe"],
    })

    let stdout = ""
    let stderr = ""
    child.stdout.setEncoding("utf8")
    child.stderr.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk
    })
    child.on("error", rejectPromise)
    child.on("close", (status) => {
      resolvePromise({
        status: status ?? 1,
        stdout,
        stderr,
      })
    })

    if (options.stdin && options.stdin !== "ignore") {
      child.stdin.end(options.stdin)
    } else {
      child.stdin.end()
    }
  })
}

/** Resolves and canonicalizes an existing filesystem path. */
async function normalizePath(path: string) {
  return await realpath(resolve(path))
}

/** Tests whether child is equal to or nested under parent. */
function isInsideOrEqual(parent: string, child: string) {
  const rel = relative(parent, child)
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel))
}

/** Checks whether one filesystem path currently exists. */
async function pathExists(path: string) {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

/** Checks whether a thrown value is a Node filesystem error with the requested code. */
function isNodeErrorWithCode(error: unknown, code: string) {
  return (
    error instanceof Error &&
    "code" in error &&
    typeof error.code === "string" &&
    error.code === code
  )
}

/** Checks whether a local process id still exists. */
function isProcessAlive(pid: number) {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

class UserError extends Error {
  readonly status
  readonly exitCode

  constructor(message: string, status: ReviewSyncStatus = "error", exitCode = 1) {
    super(message)
    this.status = status
    this.exitCode = exitCode
  }
}

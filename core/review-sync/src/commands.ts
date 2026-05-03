/** CLI-compatible review-sync command runner. */
import { watch, type FSWatcher } from "node:fs"
import { join } from "node:path"
import { autocomplete, cancel, isCancel } from "@clack/prompts"
import { command, flag, optional, positional, runSafely, string, subcommands } from "cmd-ts"

import { createErrorResult, createReviewSyncResult, UserError } from "./errors.ts"
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
  resolveRequiredGitDir,
  resolveRequiredRepoRoot,
} from "./git.ts"
import { withSessionLock } from "./lock.ts"
import { createRuntimeContext, writeResult } from "./runtime.ts"
import {
  AgentBranchWorktreeMissingError,
  AgentWorktreeCheckoutMismatchError,
  createSessionForStart,
  inferSession,
  listAgentWorktreeChoicesForStart,
  prepareReviewBranchForStart,
} from "./session.ts"
import { createSnapshotTree } from "./snapshot.ts"
import {
  appendEvent,
  countPatchFiles,
  listSessions,
  readSessionState,
  resolveSessionDir,
  writeSessionState,
} from "./state.ts"
import { refreshReviewWorktreeFromAgentBranchRef, syncSession } from "./sync.ts"
import {
  reviewBranchPrefix,
  type ReviewSyncCommand,
  type ReviewSyncResult,
  type ReviewSyncWorktreeInput,
  type RuntimeContext,
  type SessionState,
  type StartReviewSyncInput,
  type StatusReviewSyncInput,
  type WatchReviewSyncInput,
} from "./types.ts"

const watchDebounceMs = 100

/** Session identifiers attached to verbose watch diagnostics when available. */
type WatchVerboseSession = Pick<SessionState, "sessionId" | "reviewBranch">

/** Emits verbose watch diagnostics through the regular result callback. */
type WatchVerboseEmitter = (message: string, session?: WatchVerboseSession) => Promise<void>

/** Filesystem event details retained until the watch loop can log them in order. */
type WatchEventDetail = {
  source: "worktree" | "git"
  path: string
  recursive: boolean
  eventType: string
  filename: string | null
}

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
    startReviewSyncOperation(input.agentBranch, createRuntimeContext(input.cwd)),
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

/** Watches both worktrees and runs sync when either snapshot changes. */
export async function watchReviewSession(input: WatchReviewSyncInput) {
  return await runCommandSafely("watch", () =>
    watchReviewSessionOperation(input, createRuntimeContext(input.cwd)),
  )
}

/** Performs the start workflow after CLI parsing and command-level error handling. */
async function startReviewSyncOperation(agentBranch: string, context: RuntimeContext) {
  const { result } = await startReviewSyncOperationWithSession(agentBranch, context)
  return result
}

/** Performs the start workflow and keeps the loaded session for command composition. */
async function startReviewSyncOperationWithSession(agentBranch: string, context: RuntimeContext) {
  const session = await createSessionForStart(agentBranch, context)
  await prepareReviewBranchForStart(session, context)
  const syncResult = await syncSession(session, context)

  const result = createReviewSyncResult({
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
  return { session, result }
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
  const latest = await pauseSession(session)

  return createReviewSyncResult({
    exitCode: 0,
    command: "pause",
    status: "paused",
    sessionId: latest.sessionId,
    reviewBranch: latest.reviewBranch,
    message: `Paused review sync for ${latest.reviewBranch}.`,
  })
}

/** Marks a known session paused without relying on branch-based session inference. */
async function pauseSession(session: SessionState) {
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
    return latest
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

/** Performs the watch workflow after CLI parsing and command-level error handling. */
async function watchReviewSessionOperation(input: WatchReviewSyncInput, context: RuntimeContext) {
  const emitVerbose = createWatchVerboseEmitter(input)
  await emitVerbose(
    input.agentBranch
      ? `resolving session for ${input.agentBranch}.`
      : `inferring session from ${context.cwd}.`,
  )

  const startedWorktree = await resolveRequiredRepoRoot(context.cwd, context)
  const startedBranch = await resolveCurrentBranch(startedWorktree, context)
  await emitVerbose(
    startedBranch
      ? `started from ${startedWorktree} on ${startedBranch}.`
      : `started from ${startedWorktree} with detached HEAD.`,
  )

  const resolution = await resolveSessionForWatch(
    input,
    context,
    emitVerbose,
    startedWorktree,
    startedBranch,
  )
  if (resolution.kind === "stopped") {
    return resolution.result
  }

  const { session, startResult, refreshFromBranchRefOnStart } = resolution
  await emitVerbose(`preparing ${session.reviewBranch} in ${session.reviewWorktree}.`, session)
  await prepareReviewBranchForStart(session, context)
  let fingerprint = await createWatchFingerprint(session, context)
  await emitVerbose(
    `captured initial fingerprint for ${session.agentBranch} and ${session.reviewBranch}.`,
    session,
  )
  const warningState = { pendingHumanPatchWarningSent: false }
  const events = createWatchEventQueue(input.signal)
  const watchers = await createReviewSyncWatchers(session, context, events, emitVerbose)
  let watchError: unknown

  try {
    if (startResult) {
      await input.onResult?.(startResult)
    }

    await input.onResult?.(
      createReviewSyncResult({
        exitCode: 0,
        command: "watch",
        status: session.paused ? "paused" : "ok",
        sessionId: session.sessionId,
        reviewBranch: session.reviewBranch,
        message: `Watching review sync for ${session.agentBranch} -> ${session.reviewBranch}.`,
      }),
    )
    await emitVerbose("watchers are armed; waiting for changes.", session)

    if (refreshFromBranchRefOnStart) {
      await emitVerbose(
        `refreshing ${session.reviewBranch} from ${session.agentBranch} branch ref because the recorded agent checkout is unavailable.`,
        session,
      )
      await refreshReviewWorktreeFromAgentBranchRefForWatch(
        session,
        context,
        input.onResult,
        warningState,
        emitVerbose,
      )
      fingerprint = await createWatchFingerprint(await readSessionState(session), context)
      await emitVerbose("updated fingerprint after branch-ref refresh.", session)
    }

    while (await events.waitForEvent()) {
      if (!(await waitForWatchQuietPeriod(events, input.signal))) {
        break
      }
      await emitQueuedWatchEvents(emitVerbose, events, session)

      const latest = await readSessionState(session)
      const nextFingerprint = await createWatchFingerprint(latest, context)
      await emitVerbose("compared current fingerprint with the last handled fingerprint.", latest)
      if (nextFingerprint === fingerprint) {
        await emitVerbose("fingerprint unchanged; no sync needed.", latest)
        continue
      }

      await emitVerbose(
        "fingerprint changed; running sync when the agent checkout is ready.",
        latest,
      )
      const result = await syncForWatchWhenAgentCheckoutReady(
        latest,
        context,
        events,
        input.signal,
        input.onResult,
        warningState,
        emitVerbose,
      )
      if (!result) {
        break
      }
      await input.onResult?.(result)
      if (isAbortSignalAborted(input.signal)) {
        break
      }
      fingerprint = await createWatchFingerprint(await readSessionState(session), context)
    }
  } catch (error) {
    watchError = error
  } finally {
    await emitVerbose(
      `closing ${watchers.length} filesystem watcher${watchers.length === 1 ? "" : "s"}.`,
      session,
    )
    closeReviewSyncWatchers(watchers)
  }

  await emitVerbose(
    "pausing session and restoring the starting review branch when needed.",
    session,
  )
  const cleanup = await cleanupWatchExit({
    session,
    context,
    startedWorktree,
    startedBranch,
  })
  await emitVerbose(
    cleanup.failures.length > 0
      ? `watch cleanup finished with ${cleanup.failures.length} failure${cleanup.failures.length === 1 ? "" : "s"}.`
      : "watch cleanup complete.",
    cleanup.latest,
  )

  if (watchError) {
    if (cleanup.failures.length > 0) {
      throw new UserError(
        [
          formatThrownError(watchError),
          "Additionally, watch cleanup did not complete:",
          ...cleanup.failures.map((failure) => `- ${failure}`),
        ].join("\n"),
      )
    }
    throw watchError
  }

  return createReviewSyncResult({
    exitCode:
      cleanup.failures.length > 0
        ? getWatchCleanupFailureExitCode(input.signal)
        : getWatchExitCode(input.signal),
    command: "watch",
    status: cleanup.failures.length > 0 ? "error" : "paused",
    sessionId: cleanup.latest.sessionId,
    reviewBranch: cleanup.latest.reviewBranch,
    message: formatWatchStoppedMessage(cleanup),
  })
}

/** Pauses watch state and restores the review worktree branch that launched watch. */
async function cleanupWatchExit(input: {
  session: SessionState
  context: RuntimeContext
  startedWorktree: string
  startedBranch: string | null
}) {
  const failures: string[] = []
  const notes: string[] = []
  let latest = input.session
  let paused = false

  try {
    latest = await pauseSession(input.session)
    paused = true
  } catch (error) {
    failures.push(`Could not pause review sync: ${formatThrownError(error)}`)
    try {
      latest = await readSessionState(input.session)
    } catch {
      latest = input.session
    }
  }

  if (!paused) {
    return { latest, notes, failures }
  }

  if (input.startedWorktree === latest.reviewWorktree) {
    if (input.startedBranch) {
      try {
        const currentBranch = await resolveCurrentBranch(latest.reviewWorktree, input.context)
        if (currentBranch !== input.startedBranch) {
          await git(latest.reviewWorktree, ["checkout", input.startedBranch], input.context, {
            stdin: "ignore",
          })
        }
        notes.push(`Checked out ${input.startedBranch}.`)
      } catch (error) {
        failures.push(
          `Could not check out ${input.startedBranch} in ${latest.reviewWorktree}: ${formatThrownError(error)}`,
        )
      }
    } else {
      notes.push("No starting branch was checked out, so no branch was restored.")
    }
  }

  return { latest, notes, failures }
}

/** Formats the final watch result with any cleanup outcome the user must act on. */
function formatWatchStoppedMessage(cleanup: Awaited<ReturnType<typeof cleanupWatchExit>>) {
  const stopped = `Stopped watching review sync for ${cleanup.latest.reviewBranch}.`
  if (cleanup.failures.length > 0) {
    return [
      `${stopped} Cleanup did not complete:`,
      ...cleanup.failures.map((failure) => `- ${failure}`),
    ].join("\n")
  }
  if (cleanup.notes.length > 0) {
    return `${stopped} Paused review sync. ${cleanup.notes.join(" ")}`
  }
  return `${stopped} Paused review sync.`
}

/** Keeps signal exits conventional unless cleanup is the only failure. */
function getWatchCleanupFailureExitCode(signal: AbortSignal | undefined) {
  const signalExitCode = getWatchExitCode(signal)
  return signalExitCode === 0 ? 1 : signalExitCode
}

/** Returns a concise user-facing message for thrown values. */
function formatThrownError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
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
          agentBranch: positional({
            type: optional(string),
            displayName: "agent-branch",
            description: "Agent branch checked out in another worktree",
          }),
        },
        handler: async ({ agentBranch }) => {
          const context = createRuntimeContext(cwd)
          return await runCommandSafely("start", async () =>
            startReviewSyncOperation(agentBranch ?? (await promptForAgentBranch(context)), context),
          )
        },
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
      watch: command({
        name: "watch",
        description: "Continuously sync when the agent or review worktree changes",
        args: {
          agentBranch: positional({
            type: optional(string),
            displayName: "agent-branch",
            description: "Agent branch checked out in another worktree",
          }),
          verbose: flag({
            long: "verbose",
            description:
              "Print watch diagnostics for session resolution, filesystem events, and sync decisions",
          }),
        },
        handler: async ({ agentBranch, verbose }) => {
          const abort = createProcessAbortSignal()
          try {
            return await watchReviewSession({
              cwd,
              agentBranch,
              signal: abort.signal,
              verbose,
              onResult: writeResult,
            })
          } finally {
            abort.cleanup()
          }
        },
      }),
    },
  })
}

/** Resolves an existing watch session or creates it from start-compatible input. */
async function resolveSessionForWatch(
  input: WatchReviewSyncInput,
  context: RuntimeContext,
  emitVerbose: WatchVerboseEmitter,
  startedWorktree: string,
  startedBranch: string | null,
) {
  if (input.agentBranch) {
    return await resolveAgentBranchSessionForWatch(
      {
        ...input,
        agentBranch: input.agentBranch,
      },
      context,
      emitVerbose,
      startedWorktree,
      startedBranch,
    )
  }

  await emitVerbose("inferring saved session from the current worktree.")
  const session = await inferSession(context)
  await emitVerbose(`inferred session ${session.sessionId} for ${session.agentBranch}.`, session)
  return {
    kind: "ready" as const,
    session,
    startResult: null,
    refreshFromBranchRefOnStart: false,
  }
}

/** Waits for Git metadata to show that the requested agent branch owns a worktree. */
async function resolveAgentBranchSessionForWatch(
  input: WatchReviewSyncInput & { agentBranch: string },
  context: RuntimeContext,
  emitVerbose: WatchVerboseEmitter,
  startedWorktree: string,
  startedBranch: string | null,
) {
  const events = createWatchEventQueue(input.signal)
  const watchers = await createRepositoryMetadataWatchers(context, events, emitVerbose)
  let waitingResultSent = false
  let preparedReviewBranch: string | null = null
  let branchRefPreparationCurrent = false
  let lastBranchRefPreparation: BranchRefPreparation | null = null

  try {
    await emitVerbose(
      `looking for an existing session or checked-out worktree for ${input.agentBranch}.`,
    )
    while (!isAbortSignalAborted(input.signal)) {
      const existingSession = await findExistingAgentBranchSessionForWatch(
        input.agentBranch,
        context,
      )
      if (existingSession && (await isRecordedAgentCheckoutUnavailable(existingSession, context))) {
        await emitVerbose(
          `reusing session ${existingSession.sessionId}; recorded agent worktree ${existingSession.agentWorktree} is not on ${existingSession.agentBranch}.`,
          existingSession,
        )
        return {
          kind: "ready" as const,
          session: existingSession,
          startResult: null,
          refreshFromBranchRefOnStart: true,
        }
      }

      try {
        await emitVerbose(`attempting to start session for ${input.agentBranch}.`)
        const { session, result } = await startReviewSyncOperationWithSession(
          input.agentBranch,
          context,
        )
        await emitVerbose(
          `session ready with agent worktree ${session.agentWorktree} and review worktree ${session.reviewWorktree}.`,
          session,
        )
        return {
          kind: "ready" as const,
          session,
          startResult: result,
          refreshFromBranchRefOnStart: false,
        }
      } catch (error) {
        if (!(error instanceof AgentBranchWorktreeMissingError)) {
          throw error
        }

        const existingSession = await findExistingAgentBranchSessionForWatch(
          input.agentBranch,
          context,
        )
        if (existingSession) {
          await emitVerbose(
            `reusing saved session ${existingSession.sessionId} while the agent checkout is unavailable.`,
            existingSession,
          )
          return {
            kind: "ready" as const,
            session: existingSession,
            startResult: null,
            refreshFromBranchRefOnStart: true,
          }
        }

        let preparedThisIteration = false
        if (!branchRefPreparationCurrent) {
          lastBranchRefPreparation = await prepareReviewBranchFromAgentBranchRefForWatch(
            input.agentBranch,
            context,
            emitVerbose,
          )
          branchRefPreparationCurrent = true
          preparedThisIteration = true
          if (lastBranchRefPreparation.status === "prepared") {
            preparedReviewBranch = lastBranchRefPreparation.reviewBranch
          }
          if (lastBranchRefPreparation.generatedEvents) {
            await discardSelfGeneratedWatchEvents(events, input.signal, emitVerbose)
          }
        }

        if (!waitingResultSent) {
          waitingResultSent = true
          await emitVerbose(
            `no checked-out agent worktree found for ${input.agentBranch}; waiting for repository metadata.`,
          )
          await input.onResult?.(
            createReviewSyncResult({
              exitCode: 0,
              command: "watch",
              status: "ok",
              message: formatAgentCheckoutWaitingMessage(
                input.agentBranch,
                lastBranchRefPreparation,
              ),
            }),
          )
        }

        if (preparedThisIteration && lastBranchRefPreparation?.status === "prepared") {
          continue
        }

        if (
          !(await events.waitForEvent()) ||
          !(await waitForWatchQuietPeriod(events, input.signal))
        ) {
          break
        }
        await emitQueuedWatchEvents(emitVerbose, events)
        branchRefPreparationCurrent = false
        await emitVerbose(`repository metadata changed; retrying ${input.agentBranch}.`)
      }
    }
  } finally {
    await emitVerbose(
      `closing ${watchers.length} repository metadata watcher${watchers.length === 1 ? "" : "s"}.`,
    )
    closeReviewSyncWatchers(watchers)
  }

  const restoreNote = preparedReviewBranch
    ? await restorePreparedBranchRefPreviewForWatch({
        context,
        startedWorktree,
        startedBranch,
        preparedReviewBranch,
        emitVerbose,
      })
    : null

  return {
    kind: "stopped" as const,
    result: createReviewSyncResult({
      exitCode: getWatchExitCode(input.signal),
      command: "watch",
      status: "ok",
      message: [
        `Stopped waiting for ${input.agentBranch} to be checked out in an agent worktree.`,
        restoreNote,
      ]
        .filter(Boolean)
        .join(" "),
    }),
  }
}

/** Outcome of preparing a review branch from an agent branch ref before a session exists. */
type BranchRefPreparation =
  | {
      status: "prepared"
      reviewBranch: string
      generatedEvents: boolean
    }
  | {
      status: "skipped"
      reason: "dirty-review-worktree" | "missing-agent-branch-ref" | "unsupported-git-state"
      reviewBranch: string
      generatedEvents: boolean
    }

/** Prepares the human review branch from the agent branch ref while no agent checkout exists. */
async function prepareReviewBranchFromAgentBranchRefForWatch(
  agentBranch: string,
  context: RuntimeContext,
  emitVerbose: WatchVerboseEmitter,
) {
  const reviewWorktree = await resolveRequiredRepoRoot(context.cwd, context)
  const reviewBranch = `${reviewBranchPrefix}${agentBranch}`

  try {
    await assertSupportedGitState(reviewWorktree, context)
  } catch (error) {
    if (!(error instanceof UserError)) {
      throw error
    }
    await emitVerbose(
      `branch-ref preparation skipped for ${reviewBranch}: unsupported Git state in ${reviewWorktree}.`,
    )
    return {
      status: "skipped",
      reason: "unsupported-git-state",
      reviewBranch,
      generatedEvents: false,
    } satisfies BranchRefPreparation
  }

  const branchHead = await resolveRef(reviewWorktree, `refs/heads/${agentBranch}`, context)
  if (!branchHead) {
    await emitVerbose(
      `branch-ref preparation skipped for ${reviewBranch}: ${agentBranch} branch ref is missing.`,
    )
    return {
      status: "skipped",
      reason: "missing-agent-branch-ref",
      reviewBranch,
      generatedEvents: false,
    } satisfies BranchRefPreparation
  }

  const currentBranch = await resolveCurrentBranch(reviewWorktree, context)
  const currentHead = await resolveRef(reviewWorktree, "HEAD", context)
  if (currentBranch === reviewBranch && currentHead === branchHead) {
    await emitVerbose(
      `${reviewBranch} in ${reviewWorktree} is already prepared from ${agentBranch} branch ref.`,
    )
    return {
      status: "prepared",
      reviewBranch,
      generatedEvents: false,
    } satisfies BranchRefPreparation
  }

  await assertReviewBranchNotCheckedOutElsewhere({
    cwd: reviewWorktree,
    reviewBranch,
    reviewWorktree,
    context,
  })

  if (!(await isWorktreeClean(reviewWorktree, context))) {
    await emitVerbose(
      `branch-ref preparation skipped for ${reviewBranch}: ${reviewWorktree} has local changes.`,
    )
    return {
      status: "skipped",
      reason: "dirty-review-worktree",
      reviewBranch,
      generatedEvents: true,
    } satisfies BranchRefPreparation
  }

  if (currentBranch !== reviewBranch) {
    if (await branchExists(reviewWorktree, reviewBranch, context)) {
      await git(reviewWorktree, ["checkout", reviewBranch], context, {
        stdin: "ignore",
      })
    } else {
      await git(reviewWorktree, ["branch", reviewBranch, branchHead], context)
      await git(reviewWorktree, ["checkout", reviewBranch], context, {
        stdin: "ignore",
      })
    }
  }

  const preparedHead = await resolveRef(reviewWorktree, "HEAD", context)
  if (preparedHead !== branchHead) {
    await git(reviewWorktree, ["reset", "--hard", branchHead], context)
    await git(reviewWorktree, ["clean", "-fd"], context)
  }

  await emitVerbose(`prepared ${reviewBranch} in ${reviewWorktree} from ${agentBranch} branch ref.`)
  return {
    status: "prepared",
    reviewBranch,
    generatedEvents: true,
  } satisfies BranchRefPreparation
}

/** Drops Git metadata events produced by branch-ref preparation before waiting. */
async function discardSelfGeneratedWatchEvents(
  events: WatchEventQueue,
  signal: AbortSignal | undefined,
  emitVerbose: WatchVerboseEmitter,
) {
  if (!(await events.waitForEventOrTimeout(watchDebounceMs))) {
    return
  }

  await waitForWatchQuietPeriod(events, signal)
  const discarded = events.drainEvents()
  if (discarded.length === 0) {
    return
  }

  await emitVerbose(
    `ignored ${discarded.length} filesystem event${discarded.length === 1 ? "" : "s"} generated while preparing the review branch.`,
  )
}

/** Restores the starting branch if watch stops before a durable session exists. */
async function restorePreparedBranchRefPreviewForWatch(input: {
  context: RuntimeContext
  startedWorktree: string
  startedBranch: string | null
  preparedReviewBranch: string
  emitVerbose: WatchVerboseEmitter
}) {
  if (!input.startedBranch) {
    return "No starting branch was checked out, so no branch was restored."
  }

  const currentBranch = await resolveCurrentBranch(input.startedWorktree, input.context)
  if (currentBranch !== input.preparedReviewBranch) {
    return null
  }

  if (!(await isWorktreeClean(input.startedWorktree, input.context))) {
    return `Left ${input.preparedReviewBranch} checked out because the review worktree has local changes.`
  }

  await git(input.startedWorktree, ["checkout", input.startedBranch], input.context, {
    stdin: "ignore",
  })
  await input.emitVerbose(
    `restored ${input.startedBranch} after stopping before a session was ready.`,
  )
  return `Checked out ${input.startedBranch}.`
}

/** Describes the safe branch-ref preparation state in the first waiting message. */
function formatAgentCheckoutWaitingMessage(
  agentBranch: string,
  branchRefPreparation: BranchRefPreparation | null,
) {
  const waiting = `Waiting for ${agentBranch} to be checked out in an agent worktree.`
  if (branchRefPreparation?.status === "prepared") {
    return `${waiting} Checked out ${branchRefPreparation.reviewBranch} from the ${agentBranch} branch ref.`
  }
  if (branchRefPreparation?.reason === "dirty-review-worktree") {
    return `${waiting} Review worktree has local changes, so ${branchRefPreparation.reviewBranch} was not checked out.`
  }
  if (branchRefPreparation?.reason === "unsupported-git-state") {
    return `${waiting} Review worktree has an in-progress Git operation, so ${branchRefPreparation.reviewBranch} was not checked out.`
  }
  return waiting
}

/** Finds a saved session for explicit watch when the agent branch is temporarily unavailable. */
async function findExistingAgentBranchSessionForWatch(
  agentBranch: string,
  context: RuntimeContext,
) {
  const repoRoot = await resolveRequiredRepoRoot(context.cwd, context)
  const [repoCommonDir, cwd] = await Promise.all([
    resolveRequiredGitCommonDir(repoRoot, context),
    normalizePath(context.cwd),
  ])
  const matches = (await listSessions(repoCommonDir)).filter((session) => {
    if (session.agentBranch !== agentBranch) {
      return false
    }
    return (
      isInsideOrEqual(session.agentWorktree, cwd) || isInsideOrEqual(session.reviewWorktree, cwd)
    )
  })

  if (matches.length === 1) {
    return matches[0]!
  }
  if (matches.length > 1) {
    throw new UserError(
      [
        `Multiple review-sync sessions for ${agentBranch} match ${cwd}.`,
        "Run watch from a worktree recorded by only one session, or move stale session dirs out of the Git common directory.",
        "Matching sessions:",
        ...matches.map((session) => `- ${session.sessionId}: ${session.reviewBranch}`),
      ].join("\n"),
    )
  }

  return null
}

/** Checks the saved agent worktree without treating other worktrees as session owners. */
async function isRecordedAgentCheckoutUnavailable(session: SessionState, context: RuntimeContext) {
  return (await resolveCurrentBranch(session.agentWorktree, context)) !== session.agentBranch
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

/** Runs watch-triggered syncs once the agent returns to the recorded branch. */
async function syncForWatchWhenAgentCheckoutReady(
  session: SessionState,
  context: RuntimeContext,
  events: WatchEventQueue,
  signal: AbortSignal | undefined,
  onResult: WatchReviewSyncInput["onResult"],
  warningState: { pendingHumanPatchWarningSent: boolean },
  emitVerbose: WatchVerboseEmitter,
) {
  while (!isAbortSignalAborted(signal)) {
    try {
      return await syncReviewSessionOperation(context)
    } catch (error) {
      if (!(error instanceof AgentWorktreeCheckoutMismatchError)) {
        return createErrorResult("sync", error)
      }
      if (
        !(await waitForExpectedAgentCheckout(
          session,
          error,
          context,
          events,
          signal,
          onResult,
          warningState,
          emitVerbose,
        ))
      ) {
        return null
      }
    }
  }

  return null
}

/** Waits on watch events until the recorded agent branch is checked out again. */
async function waitForExpectedAgentCheckout(
  session: SessionState,
  mismatch: AgentWorktreeCheckoutMismatchError,
  context: RuntimeContext,
  events: WatchEventQueue,
  signal: AbortSignal | undefined,
  onResult: WatchReviewSyncInput["onResult"],
  warningState: { pendingHumanPatchWarningSent: boolean },
  emitVerbose: WatchVerboseEmitter,
) {
  while (!isAbortSignalAborted(signal)) {
    const branch = await resolveCurrentBranch(mismatch.worktree, context)
    if (branch === mismatch.expectedBranch) {
      await emitVerbose(
        `agent worktree ${mismatch.worktree} returned to ${mismatch.expectedBranch}.`,
        session,
      )
      return true
    }

    await emitVerbose(
      `agent worktree ${mismatch.worktree} is on ${branch ?? "detached HEAD"}; waiting for ${mismatch.expectedBranch}.`,
      session,
    )
    await refreshReviewWorktreeFromAgentBranchRefForWatch(
      session,
      context,
      onResult,
      warningState,
      emitVerbose,
    )

    if (!(await events.waitForEvent()) || !(await waitForWatchQuietPeriod(events, signal))) {
      return false
    }
    await emitQueuedWatchEvents(emitVerbose, events, session)
  }

  return false
}

/** Refreshes from the agent branch ref and emits the watch warning only once. */
async function refreshReviewWorktreeFromAgentBranchRefForWatch(
  session: SessionState,
  context: RuntimeContext,
  onResult: WatchReviewSyncInput["onResult"],
  warningState: { pendingHumanPatchWarningSent: boolean },
  emitVerbose: WatchVerboseEmitter,
) {
  await emitVerbose(
    `checking whether ${session.reviewBranch} can refresh from ${session.agentBranch} branch ref.`,
    session,
  )
  const refreshResult = await refreshReviewWorktreeFromAgentBranchRef(session, context)
  if (refreshResult.status === "refreshed") {
    await emitVerbose(
      `refreshed ${session.reviewBranch} from ${session.agentBranch} branch ref.`,
      session,
    )
  } else {
    await emitVerbose(`branch-ref refresh skipped: ${refreshResult.reason}.`, session)
  }
  if (
    refreshResult.status === "skipped" &&
    refreshResult.reason === "pending-human-patch" &&
    !warningState.pendingHumanPatchWarningSent
  ) {
    warningState.pendingHumanPatchWarningSent = true
    await onResult?.(
      createReviewSyncResult({
        exitCode: 0,
        command: "watch",
        status: "ok",
        sessionId: session.sessionId,
        reviewBranch: session.reviewBranch,
        message: `Warning: review refresh skipped while waiting for ${session.agentBranch}; ${session.reviewWorktree} has unapplied human edits.`,
      }),
    )
  }
}

/** Builds a content fingerprint that changes for commits, branch moves, and dirty files. */
async function createWatchFingerprint(
  session: { agentWorktree: string; reviewWorktree: string; agentBranch: string },
  context: RuntimeContext,
) {
  const [agent, review, agentBranchHead] = await Promise.all([
    createWorktreeFingerprint(session.agentWorktree, context),
    createWorktreeFingerprint(session.reviewWorktree, context),
    resolveRef(session.reviewWorktree, `refs/heads/${session.agentBranch}`, context),
  ])
  return JSON.stringify({ agent, review, agentBranchHead })
}

/** Captures the branch, HEAD, and snapshot tree for one worktree. */
async function createWorktreeFingerprint(cwd: string, context: RuntimeContext) {
  const [branch, head, tree] = await Promise.all([
    git(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"], context, {
      allowFailure: true,
    }),
    git(cwd, ["rev-parse", "HEAD"], context),
    createSnapshotTree({
      cwd,
      context,
    }),
  ])

  return {
    branch: branch.status === 0 ? branch.stdout.trim() : null,
    head: head.stdout.trim(),
    tree,
  }
}

/** Creates event watchers for worktree contents, checked-out HEADs, and branch refs. */
async function createReviewSyncWatchers(
  session: {
    agentWorktree: string
    reviewWorktree: string
    repoCommonDir: string
  },
  context: RuntimeContext,
  events: WatchEventQueue,
  emitVerbose: WatchVerboseEmitter,
) {
  const [agentGitDir, reviewGitDir] = await Promise.all([
    resolveRequiredGitDir(session.agentWorktree, context),
    resolveRequiredGitDir(session.reviewWorktree, context),
  ])
  const targets = await resolveExistingWatchTargets([
    {
      path: session.agentWorktree,
      recursive: true,
      source: "worktree",
    },
    {
      path: session.reviewWorktree,
      recursive: true,
      source: "worktree",
    },
    {
      path: agentGitDir,
      recursive: false,
      source: "git",
    },
    {
      path: reviewGitDir,
      recursive: false,
      source: "git",
    },
    {
      path: session.repoCommonDir,
      recursive: false,
      source: "git",
    },
    {
      path: join(session.repoCommonDir, "refs"),
      recursive: true,
      source: "git",
    },
  ])
  await emitVerbose(`watching paths: ${formatWatchTargets(targets)}.`)

  return await createFsWatchers(targets, events)
}

/** Watches repository-level Git metadata before a session knows its agent worktree. */
async function createRepositoryMetadataWatchers(
  context: RuntimeContext,
  events: WatchEventQueue,
  emitVerbose: WatchVerboseEmitter,
) {
  const repoRoot = await resolveRequiredRepoRoot(context.cwd, context)
  const [commonDir, gitDir] = await Promise.all([
    resolveRequiredGitCommonDir(repoRoot, context),
    resolveRequiredGitDir(repoRoot, context),
  ])
  const targets = await resolveExistingWatchTargets([
    {
      path: gitDir,
      recursive: false,
      source: "git",
    },
    {
      path: commonDir,
      recursive: false,
      source: "git",
    },
    {
      path: join(commonDir, "refs"),
      recursive: true,
      source: "git",
    },
    {
      path: join(commonDir, "worktrees"),
      recursive: true,
      source: "git",
    },
  ])
  await emitVerbose(`watching repository metadata paths: ${formatWatchTargets(targets)}.`)

  return await createFsWatchers(targets, events)
}

/** Creates filesystem watchers for an already resolved set of existing targets. */
async function createFsWatchers(
  targets: Array<{ path: string; recursive: boolean; source: "worktree" | "git" }>,
  events: WatchEventQueue,
) {
  const watchers: FSWatcher[] = []
  try {
    for (const target of targets) {
      const watcher = watch(target.path, { recursive: target.recursive }, (eventType, filename) => {
        if (shouldIgnoreWatchEvent(target.source, filename)) {
          return
        }
        events.notify({
          source: target.source,
          path: target.path,
          recursive: target.recursive,
          eventType,
          filename: filename?.toString() ?? null,
        })
      })
      watcher.on("error", events.fail)
      watchers.push(watcher)
    }
  } catch (error) {
    closeReviewSyncWatchers(watchers)
    throw error
  }

  return watchers
}

/** Keeps one watcher per existing path so duplicate Git dirs do not duplicate events. */
async function resolveExistingWatchTargets(
  targets: Array<{ path: string; recursive: boolean; source: "worktree" | "git" }>,
) {
  const seen = new Set<string>()
  const existing: Array<{ path: string; recursive: boolean; source: "worktree" | "git" }> = []

  for (const target of targets) {
    if (seen.has(target.path) || !(await pathExists(target.path))) {
      continue
    }

    seen.add(target.path)
    existing.push(target)
  }

  return existing
}

/** Avoids routing duplicate Git metadata events from the main worktree content watcher. */
function shouldIgnoreWatchEvent(source: "worktree" | "git", filename: string | Buffer | null) {
  if (source !== "worktree" || filename === null) {
    return false
  }

  const path = filename.toString()
  return path === ".git" || path.startsWith(".git/") || path.startsWith(".git\\")
}

/** Stops all filesystem watchers associated with one watch command. */
function closeReviewSyncWatchers(watchers: FSWatcher[]) {
  for (const watcher of watchers) {
    watcher.close()
  }
}

/** Creates a no-op logger unless watch verbose output was requested. */
function createWatchVerboseEmitter(input: WatchReviewSyncInput) {
  return async (message: string, session?: WatchVerboseSession) => {
    if (!input.verbose) {
      return
    }

    await input.onResult?.(
      createReviewSyncResult({
        exitCode: 0,
        command: "watch",
        status: "ok",
        sessionId: session?.sessionId,
        reviewBranch: session?.reviewBranch,
        verbose: true,
        message: `Verbose: ${message}`,
      }),
    )
  }
}

/** Emits a compact summary of filesystem events coalesced into one watch cycle. */
async function emitQueuedWatchEvents(
  emitVerbose: WatchVerboseEmitter,
  events: WatchEventQueue,
  session?: WatchVerboseSession,
) {
  const queued = events.drainEvents()
  if (queued.length === 0) {
    await emitVerbose("received a watch signal without filesystem event details.", session)
    return
  }

  const shown = queued.slice(0, 5).map(formatWatchEvent)
  const suffix = queued.length > shown.length ? `; +${queued.length - shown.length} more` : ""
  await emitVerbose(
    `coalesced ${queued.length} filesystem event${queued.length === 1 ? "" : "s"}: ${shown.join("; ")}${suffix}.`,
    session,
  )
}

/** Formats watcher targets in the same compact style as verbose event summaries. */
function formatWatchTargets(
  targets: Array<{ path: string; recursive: boolean; source: "worktree" | "git" }>,
) {
  return targets
    .map((target) => `${target.source} ${target.recursive ? "recursive " : ""}${target.path}`)
    .join("; ")
}

/** Formats one filesystem event without hiding whether it came from Git metadata. */
function formatWatchEvent(event: WatchEventDetail) {
  const changedPath = event.filename ? join(event.path, event.filename) : event.path
  return `${event.source} ${event.eventType} ${changedPath}`
}

/** Creates a small event queue that can also wake on aborts or watcher errors. */
function createWatchEventQueue(signal: AbortSignal | undefined) {
  let pending = false
  const pendingEvents: WatchEventDetail[] = []
  let failure: unknown
  const waiters = new Set<(value: boolean) => void>()

  const flushWaiters = (value: boolean) => {
    for (const waiter of waiters) {
      waiter(value)
    }
    waiters.clear()
  }

  const waitForEvent = () => waitForEventOrTimeout(null)
  const waitForEventOrTimeout = (timeoutMs: number | null) => {
    if (failure) {
      throw failure
    }
    if (pending) {
      pending = false
      return Promise.resolve(true)
    }
    if (isAbortSignalAborted(signal)) {
      return Promise.resolve(false)
    }

    return new Promise<boolean>((resolvePromise, rejectPromise) => {
      let timeout: ReturnType<typeof setTimeout> | null = null
      const done = (value: boolean) => {
        if (timeout) {
          clearTimeout(timeout)
        }
        waiters.delete(done)
        signal?.removeEventListener("abort", abort)

        if (failure) {
          rejectPromise(failure)
          return
        }
        resolvePromise(value)
      }
      const abort = () => done(false)

      waiters.add(done)
      signal?.addEventListener("abort", abort, { once: true })

      if (timeoutMs !== null) {
        timeout = setTimeout(() => done(false), timeoutMs)
      }
    })
  }

  return {
    notify: (event?: WatchEventDetail) => {
      pending = true
      if (event) {
        pendingEvents.push(event)
      }
      flushWaiters(true)
    },
    drainEvents: () => pendingEvents.splice(0),
    fail: (error: unknown) => {
      failure = error
      flushWaiters(false)
    },
    waitForEvent,
    waitForEventOrTimeout,
  }
}

type WatchEventQueue = ReturnType<typeof createWatchEventQueue>

/** Waits until filesystem events have been quiet long enough for Git to settle. */
async function waitForWatchQuietPeriod(events: WatchEventQueue, signal: AbortSignal | undefined) {
  while (!isAbortSignalAborted(signal)) {
    const changed = await events.waitForEventOrTimeout(watchDebounceMs)
    if (!changed) {
      return true
    }
  }

  return false
}

/** Checks an abort signal without causing TypeScript to over-narrow loop state. */
function isAbortSignalAborted(signal: AbortSignal | undefined) {
  return signal?.aborted === true
}

/** Translates process termination signals into conventional command exit codes. */
function getWatchExitCode(signal: AbortSignal | undefined) {
  if (signal?.reason === "SIGINT") {
    return 130
  }
  if (signal?.reason === "SIGTERM") {
    return 143
  }
  return 0
}

/** Creates an abort signal that lets the CLI clean up when interrupted. */
function createProcessAbortSignal() {
  const controller = new AbortController()
  const abortForSigint = () => controller.abort("SIGINT")
  const abortForSigterm = () => controller.abort("SIGTERM")
  process.once("SIGINT", abortForSigint)
  process.once("SIGTERM", abortForSigterm)

  return {
    signal: controller.signal,
    cleanup: () => {
      process.off("SIGINT", abortForSigint)
      process.off("SIGTERM", abortForSigterm)
    },
  }
}

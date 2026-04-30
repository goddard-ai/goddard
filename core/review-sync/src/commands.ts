/** CLI-compatible review-sync command runner. */
import { watch, type FSWatcher } from "node:fs"
import { join } from "node:path"
import { autocomplete, cancel, isCancel } from "@clack/prompts"
import { command, flag, optional, positional, runSafely, string, subcommands } from "cmd-ts"

import { createErrorResult, createReviewSyncResult, UserError } from "./errors.ts"
import { git, pathExists, resolveCurrentBranch, resolveRef, resolveRequiredGitDir } from "./git.ts"
import { withSessionLock } from "./lock.ts"
import { createRuntimeContext, writeResult } from "./runtime.ts"
import {
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
  WatchReviewSyncInput,
} from "./types.ts"

const watchDebounceMs = 100

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

/** Performs the watch workflow after CLI parsing and command-level error handling. */
async function watchReviewSessionOperation(input: WatchReviewSyncInput, context: RuntimeContext) {
  const { session, startResult } = await resolveSessionForWatch(input, context)
  let fingerprint = await createWatchFingerprint(session, context)
  const events = createWatchEventQueue(input.signal)
  const watchers = await createReviewSyncWatchers(session, context, events)

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

    while (await events.waitForEvent()) {
      if (!(await waitForWatchQuietPeriod(events, input.signal))) {
        break
      }

      const latest = await readSessionState(session)
      const nextFingerprint = await createWatchFingerprint(latest, context)
      if (nextFingerprint === fingerprint) {
        continue
      }

      const result = await syncForWatchWhenAgentCheckoutReady(context, events, input.signal)
      if (!result) {
        break
      }
      await input.onResult?.(result)
      if (isAbortSignalAborted(input.signal)) {
        break
      }
      fingerprint = await createWatchFingerprint(await readSessionState(session), context)
    }
  } finally {
    closeReviewSyncWatchers(watchers)
  }

  const latest = await readSessionState(session)
  return createReviewSyncResult({
    exitCode: getWatchExitCode(input.signal),
    command: "watch",
    status: latest.paused ? "paused" : "ok",
    sessionId: latest.sessionId,
    reviewBranch: latest.reviewBranch,
    message: `Stopped watching review sync for ${latest.reviewBranch}.`,
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
        },
        handler: async ({ agentBranch }) => {
          const abort = createProcessAbortSignal()
          try {
            return await watchReviewSession({
              cwd,
              agentBranch,
              signal: abort.signal,
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
async function resolveSessionForWatch(input: WatchReviewSyncInput, context: RuntimeContext) {
  if (input.agentBranch) {
    const { session, result } = await startReviewSyncOperationWithSession(
      input.agentBranch,
      context,
    )
    return { session, startResult: result }
  }

  return {
    session: await inferSession(context),
    startResult: null,
  }
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
  context: RuntimeContext,
  events: WatchEventQueue,
  signal: AbortSignal | undefined,
) {
  while (!isAbortSignalAborted(signal)) {
    try {
      return await syncReviewSessionOperation(context)
    } catch (error) {
      if (!(error instanceof AgentWorktreeCheckoutMismatchError)) {
        return createErrorResult("sync", error)
      }
      if (!(await waitForExpectedAgentCheckout(error, context, events, signal))) {
        return null
      }
    }
  }

  return null
}

/** Waits on watch events until the recorded agent branch is checked out again. */
async function waitForExpectedAgentCheckout(
  mismatch: AgentWorktreeCheckoutMismatchError,
  context: RuntimeContext,
  events: WatchEventQueue,
  signal: AbortSignal | undefined,
) {
  while (!isAbortSignalAborted(signal)) {
    const branch = await resolveCurrentBranch(mismatch.worktree, context)
    if (branch === mismatch.expectedBranch) {
      return true
    }

    if (!(await events.waitForEvent()) || !(await waitForWatchQuietPeriod(events, signal))) {
      return false
    }
  }

  return false
}

/** Builds a content fingerprint that changes for commits, branch moves, and dirty files. */
async function createWatchFingerprint(
  session: { agentWorktree: string; reviewWorktree: string },
  context: RuntimeContext,
) {
  const [agent, review] = await Promise.all([
    createWorktreeFingerprint(session.agentWorktree, context),
    createWorktreeFingerprint(session.reviewWorktree, context),
  ])
  return JSON.stringify({ agent, review })
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

  const watchers: FSWatcher[] = []
  try {
    for (const target of targets) {
      const watcher = watch(
        target.path,
        { recursive: target.recursive },
        (_eventType, filename) => {
          if (shouldIgnoreWatchEvent(target.source, filename)) {
            return
          }
          events.notify()
        },
      )
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

/** Creates a small event queue that can also wake on aborts or watcher errors. */
function createWatchEventQueue(signal: AbortSignal | undefined) {
  let pending = false
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
    notify: () => {
      pending = true
      flushWaiters(true)
    },
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

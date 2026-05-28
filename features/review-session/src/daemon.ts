import { realpath } from "node:fs/promises"
import { resolve } from "node:path"
import { definePlugin } from "@goddard-ai/daemon-plugin"
import {
  listReviewSessions,
  startReviewSync,
  statusReviewSession,
  stopReviewSession,
  syncReviewSession,
  watchReviewSession,
  type ReviewSyncResult,
  type ReviewSyncStatusData,
} from "@goddard-ai/review-sync"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import {
  sessionPlugin,
  type SessionEventEmitter,
  type SessionWorktreeLifecycleState,
} from "@goddard-ai/session/daemon"
import type { GetSessionWorktreeResponse } from "@goddard-ai/session/schema"
import { getErrorMessage } from "radashi"

import { reviewSessionIpcRoutes } from "./daemon-ipc.ts"
import type { ReviewSessionResponse } from "./schema.ts"

/** Narrow session feature extension surface consumed by review-session runtime orchestration. */
type SessionExtension = {
  getSession: (id: `ses_${string}`) => Promise<DaemonSession>
  getWorktree: (id: `ses_${string}`) => Promise<GetSessionWorktreeResponse>
  requireWorktree: (id: `ses_${string}`) => Promise<SessionWorktreeLifecycleState>
  listWorktrees: () => Promise<SessionWorktreeLifecycleState[]>
  findWorktreeByDir: (worktreeDir: string) => Promise<SessionWorktreeLifecycleState | null>
  isActive: (id: `ses_${string}`) => boolean
  emitDiagnostic: (id: `ses_${string}`, type: string, detail?: Record<string, unknown>) => void
  events: SessionEventEmitter
}

/** In-process watcher task for one mounted review-sync session. */
type ReviewSessionRuntime = {
  abortController: AbortController
  running: Promise<void>
}

/** Coordinates review-sync runtimes around session-owned daemon worktrees. */
function createReviewSessionManager(session: SessionExtension) {
  const runtimes = new Map<string, ReviewSessionRuntime>()
  const pendingLaunchMounts = new Set<string>()

  async function toResponse(
    id: `ses_${string}`,
    reviewSession: ReviewSyncStatusData | null,
    warnings: string[] = [],
  ) {
    const [sessionRecord, worktreeResponse] = await Promise.all([
      session.getSession(id),
      session.getWorktree(id),
    ])

    return {
      id: sessionRecord.id,
      acpSessionId: sessionRecord.acpSessionId,
      worktree: worktreeResponse.worktree,
      reviewSession,
      warnings,
    } satisfies ReviewSessionResponse
  }

  async function readReviewSessionState(worktree: SessionWorktreeLifecycleState) {
    try {
      const result = await statusReviewSession({
        cwd: worktree.worktreeDir,
        json: true,
      })
      return result.data ?? null
    } catch (error) {
      if (isMissingReviewSessionError(error)) {
        return null
      }
      throw error
    }
  }

  async function readRequiredReviewSessionState(worktree: SessionWorktreeLifecycleState) {
    const state = await readReviewSessionState(worktree)
    if (!state) {
      throw new Error(`Review session is missing for ${worktree.worktreeDir}.`)
    }
    return state
  }

  async function findMountedReviewSessionByPrimaryDir(primaryDir: string) {
    const normalizedPrimaryDir = await normalizeExistingPath(primaryDir)
    const sessions = await listReviewSessions({ cwd: normalizedPrimaryDir })
    return (
      sessions.find((reviewSession) => reviewSession.reviewWorktree === normalizedPrimaryDir) ??
      null
    )
  }

  function createReviewSessionWarnings(result: ReviewSyncResult) {
    return result.status === "rejected-human-patch" ? [result.message] : []
  }

  function emitReviewSessionWarnings(
    id: `ses_${string}`,
    reason: string,
    result: ReviewSyncResult,
  ) {
    for (const warning of createReviewSessionWarnings(result)) {
      session.emitDiagnostic(id, "review_session.warning", {
        reason,
        warning,
        acceptedPatchPath: result.acceptedPatchPath,
        rejectedPatchPath: result.rejectedPatchPath,
      })
    }
  }

  function emitReviewSessionResult(id: `ses_${string}`, reason: string, result: ReviewSyncResult) {
    if (result.status === "error") {
      session.emitDiagnostic(id, "review_session.warning", {
        reason,
        errorMessage: result.message,
      })
      return
    }
    emitReviewSessionWarnings(id, reason, result)
  }

  function isMissingReviewSessionError(error: unknown) {
    return (
      error instanceof Error &&
      error.message.includes("No review-sync session matches the current worktree.")
    )
  }

  async function normalizeExistingPath(value: string) {
    return await realpath(resolve(value))
  }

  async function stopRuntime(id: `ses_${string}`) {
    const runtime = runtimes.get(id)
    if (!runtime) {
      return
    }

    runtime.abortController.abort()
    runtimes.delete(id)
    await runtime.running.catch(() => {})
  }

  async function runCycle(id: `ses_${string}`, worktree: SessionWorktreeLifecycleState) {
    session.emitDiagnostic(id, "review_session.started", { reason: "manual" })
    const result = await syncReviewSession({ cwd: worktree.worktreeDir })
    emitReviewSessionWarnings(id, "manual", result)
    const state = await readRequiredReviewSessionState(worktree)
    session.emitDiagnostic(id, "review_session.completed", {
      reason: "manual",
      warningCount: createReviewSessionWarnings(result).length,
      lastSync: state.lastSync,
    })
    return {
      state,
      warnings: createReviewSessionWarnings(result),
    }
  }

  async function startRuntime(id: `ses_${string}`, worktree: SessionWorktreeLifecycleState) {
    await stopRuntime(id)

    const state = await readReviewSessionState(worktree)
    if (!state || !session.isActive(id)) {
      return
    }

    const abortController = new AbortController()
    const running = watchReviewSession({
      cwd: worktree.worktreeDir,
      agentBranch: worktree.branchName,
      signal: abortController.signal,
      onResult: (result) => {
        emitReviewSessionResult(id, "watch", result)
      },
    })
      .then((result) => {
        emitReviewSessionResult(id, "watch", result)
      })
      .catch((error) => {
        if (abortController.signal.aborted) {
          return
        }
        session.emitDiagnostic(id, "review_session.warning", {
          reason: "watch",
          errorMessage: getErrorMessage(error),
        })
      })

    runtimes.set(id, { abortController, running })
    session.emitDiagnostic(id, "review_session.watcher_started", {
      agentBranch: state.agentBranch,
      reviewBranch: state.reviewBranch,
    })
  }

  async function replaceMountedReviewSessionIfNeeded(
    id: `ses_${string}`,
    worktree: SessionWorktreeLifecycleState,
  ) {
    const mounted = await findMountedReviewSessionByPrimaryDir(worktree.repoRoot)
    const previousWorktree = mounted ? await session.findWorktreeByDir(mounted.agentWorktree) : null
    if (!mounted || previousWorktree?.sessionId === id) {
      return
    }

    if (previousWorktree) {
      await stopRuntime(previousWorktree.sessionId)
    }
    await stopReviewSession({ cwd: mounted.agentWorktree })

    if (previousWorktree) {
      session.emitDiagnostic(previousWorktree.sessionId, "review_session.replaced", {
        replacedBySessionId: id,
      })
    }

    session.emitDiagnostic(id, "review_session.replaced", {
      previousSessionId: previousWorktree?.sessionId ?? null,
      previousReviewSessionId: mounted.sessionId,
    })
  }

  async function mountForWorktree(id: `ses_${string}`, worktree: SessionWorktreeLifecycleState) {
    await replaceMountedReviewSessionIfNeeded(id, worktree)
    const result = await startReviewSync({
      cwd: worktree.repoRoot,
      agentBranch: worktree.branchName,
    })
    emitReviewSessionResult(id, "mount", result)
    const state = await readRequiredReviewSessionState(worktree)
    session.emitDiagnostic(id, "review_session.mounted", {
      reviewSessionId: state.sessionId,
      agentBranch: state.agentBranch,
      reviewBranch: state.reviewBranch,
    })
    return state
  }

  async function cleanupMountedReviewSession(
    id: `ses_${string}`,
    worktree: SessionWorktreeLifecycleState,
    reason: string,
  ) {
    await stopRuntime(id)
    const state = await readReviewSessionState(worktree)
    if (!state) {
      return
    }

    const result = await stopReviewSession({ cwd: worktree.worktreeDir })
    session.emitDiagnostic(id, "review_session.unmounted", {
      reason,
      reviewSessionId: result.sessionId ?? state.sessionId,
    })
  }

  return {
    async reconcilePersistedWorktrees() {
      for (const worktree of await session.listWorktrees()) {
        try {
          await cleanupMountedReviewSession(worktree.sessionId, worktree, "daemon_reconciliation")
        } catch (error) {
          session.emitDiagnostic(worktree.sessionId, "review_session.warning", {
            reason: "daemon_reconciliation",
            errorMessage: getErrorMessage(error),
          })
        }
      }
    },

    async get(id: `ses_${string}`) {
      const worktree = await session.requireWorktree(id)
      return toResponse(id, await readReviewSessionState(worktree))
    },

    async mount(id: `ses_${string}`) {
      const worktree = await session.requireWorktree(id)
      const state = await mountForWorktree(id, worktree)
      if (session.isActive(id)) {
        await startRuntime(id, worktree)
      }

      return toResponse(id, state)
    },

    async run(id: `ses_${string}`) {
      const worktree = await session.requireWorktree(id)
      session.emitDiagnostic(id, "review_session.requested", { reason: "manual" })
      const result = await runCycle(id, worktree)
      return toResponse(id, result.state, result.warnings)
    },

    async unmount(id: `ses_${string}`) {
      const worktree = await session.requireWorktree(id)
      await cleanupMountedReviewSession(id, worktree, "manual")
      return toResponse(id, null)
    },

    async close() {
      for (const id of runtimes.keys()) {
        await stopRuntime(id as `ses_${string}`)
      }
    },

    onWorktreePrepared: async (event: {
      sessionId: `ses_${string}`
      request: { worktree?: { reviewSession?: { enabled?: boolean } } }
      worktree: SessionWorktreeLifecycleState
    }) => {
      if (event.request.worktree?.reviewSession?.enabled !== true) {
        return
      }

      await mountForWorktree(event.sessionId, event.worktree)
      pendingLaunchMounts.add(event.sessionId)
    },

    onSessionActivated: async (event: {
      sessionId: `ses_${string}`
      worktree: SessionWorktreeLifecycleState | null
    }) => {
      if (!event.worktree || !pendingLaunchMounts.has(event.sessionId)) {
        return
      }

      pendingLaunchMounts.delete(event.sessionId)
      await startRuntime(event.sessionId, event.worktree)
    },

    onLaunchFinished: async (event: {
      sessionId: `ses_${string}`
      reason: string
      worktree: SessionWorktreeLifecycleState
    }) => {
      if (!pendingLaunchMounts.delete(event.sessionId)) {
        return
      }

      await cleanupMountedReviewSession(event.sessionId, event.worktree, event.reason)
    },

    onLaunchFailed: async (event: {
      sessionId: `ses_${string}`
      error: unknown
      worktree: SessionWorktreeLifecycleState
    }) => {
      if (!pendingLaunchMounts.delete(event.sessionId)) {
        return
      }

      await cleanupMountedReviewSession(event.sessionId, event.worktree, "launch_failed").catch(
        () => {},
      )
    },

    onSessionStopping: async (event: {
      sessionId: `ses_${string}`
      reason: string
      worktree: SessionWorktreeLifecycleState | null
    }) => {
      if (!event.worktree) {
        await stopRuntime(event.sessionId)
        return
      }

      await cleanupMountedReviewSession(event.sessionId, event.worktree, event.reason)
    },
  }
}

export const reviewSessionPlugin = definePlugin({
  name: "review-session",
  consumes: [sessionPlugin],
  ipcRoutes: reviewSessionIpcRoutes,
  setup({ session }) {
    const reviewSession = createReviewSessionManager(session)

    void reviewSession.reconcilePersistedWorktrees()

    session.events.on("lifecycle.worktreePrepared", reviewSession.onWorktreePrepared)
    session.events.on("lifecycle.sessionActivated", reviewSession.onSessionActivated)
    session.events.on("lifecycle.launchFinished", reviewSession.onLaunchFinished)
    session.events.on("lifecycle.launchFailed", reviewSession.onLaunchFailed)
    session.events.on("lifecycle.sessionStopping", reviewSession.onSessionStopping)

    return {
      close: () => reviewSession.close(),
      ipcHandlers: {
        reviewSession: {
          get: async ({ body: { id } }) => reviewSession.get(id),
          mount: async ({ body: { id } }) => reviewSession.mount(id),
          run: async ({ body: { id } }) => reviewSession.run(id),
          unmount: async ({ body: { id } }) => reviewSession.unmount(id),
        },
      },
    }
  },
})

import { definePlugin } from "@goddard-ai/daemon-plugin"
import type {
  CreateSessionRequest,
  SendSessionMessageRequest,
  SessionMessageEvent,
} from "@goddard-ai/schema/daemon"
import type { DaemonSessionId } from "@goddard-ai/schema/id"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import { createSessionEventEmitter } from "./daemon/events.ts"
import { createSessionManager, type SessionManager } from "./daemon/manager.ts"

export { resolveAgentProcessSpec } from "./daemon/agent-process.ts"
export { injectSystemPrompt } from "./daemon/manager.ts"
export {
  type SessionEventEmitter,
  type SessionEvents,
  type SessionWorktreeLifecycleState,
} from "./daemon/events.ts"

/** First-class session methods exposed to daemon plugins that extend session behavior. */
type SessionExtension = {
  readonly create: (request: CreateSessionRequest) => ReturnType<SessionManager["newSession"]>
  readonly workforce: SessionManager["getWorkforce"]
  readonly shutdown: SessionManager["shutdownSession"]
  readonly prompt: SessionManager["promptSession"]
  readonly recordTurnAttentionActivity: SessionManager["recordTurnAttentionActivity"]
  readonly resolveTokenScope: SessionManager["resolveTokenScope"]
  readonly allowPullRequest: SessionManager["allowPullRequest"]
  readonly getSession: SessionManager["getSession"]
  readonly getWorktree: SessionManager["getWorktree"]
  readonly requireWorktree: SessionManager["requireWorktree"]
  readonly listWorktrees: SessionManager["listWorktrees"]
  readonly findWorktreeByDir: SessionManager["findWorktreeByDir"]
  readonly isActive: SessionManager["isActive"]
  readonly emitDiagnostic: SessionManager["emitDiagnostic"]
  readonly events: ReturnType<typeof createSessionEventEmitter>
}

export const sessionPlugin = definePlugin({
  name: "session",
  ipcRoutes: sessionIpcRoutes,
  setup(context) {
    const events = createSessionEventEmitter()
    const messageListeners = new Set<(event: SessionMessageEvent) => void>()
    const sessionManager = createSessionManager({
      getDaemonUrl: context.daemonRuntime.getDaemonUrl,
      agentBinDir: context.daemonRuntime.agentBinDir,
      configManager: context.configManager,
      registryService: context.registryService,
      events,
      idleSessionShutdownTimeoutMs: context.daemonRuntime.idleSessionShutdownTimeoutMs,
      emitMessage(id, message) {
        for (const listener of messageListeners) {
          listener({ id, message })
        }
      },
    })

    async function* subscribeSessionMessages(id: DaemonSessionId, signal: AbortSignal) {
      const queue: SessionMessageEvent[] = []
      let wake: (() => void) | undefined
      const listener = (event: SessionMessageEvent) => {
        if (event.id !== id) {
          return
        }
        queue.push(event)
        wake?.()
      }
      const abort = () => {
        wake?.()
      }

      await sessionManager.sessionSubscriberConnected(id)
      messageListeners.add(listener)
      signal.addEventListener("abort", abort)
      try {
        while (!signal.aborted) {
          const event = queue.shift()
          if (event) {
            yield event
            continue
          }
          await new Promise<void>((resolve) => {
            wake = resolve
          })
          wake = undefined
        }
      } finally {
        signal.removeEventListener("abort", abort)
        messageListeners.delete(listener)
        await sessionManager.sessionSubscriberDisconnected(id)
      }
    }

    return {
      provides: {
        session: {
          create: (request) => sessionManager.newSession({ request }),
          workforce: (id) => sessionManager.getWorkforce(id),
          shutdown: (id) => sessionManager.shutdownSession(id),
          prompt: (id, prompt) => sessionManager.promptSession(id, prompt),
          recordTurnAttentionActivity: (id, metadata) =>
            sessionManager.recordTurnAttentionActivity(id, metadata),
          resolveTokenScope: (token) => sessionManager.resolveTokenScope(token),
          allowPullRequest: (id, prNumber) => sessionManager.allowPullRequest(id, prNumber),
          getSession: (id) => sessionManager.getSession(id),
          getWorktree: (id) => sessionManager.getWorktree(id),
          requireWorktree: (id) => sessionManager.requireWorktree(id),
          listWorktrees: () => sessionManager.listWorktrees(),
          findWorktreeByDir: (worktreeDir) => sessionManager.findWorktreeByDir(worktreeDir),
          isActive: (id) => sessionManager.isActive(id),
          emitDiagnostic: (id, type, detail) => sessionManager.emitDiagnostic(id, type, detail),
          events,
        } satisfies SessionExtension,
      },
      close: async () => {
        await sessionManager.close()
      },
      ipcHandlers: {
        session: {
          create: async ({ body }) => {
            const response = {
              session: await sessionManager.newSession({ request: body }),
            }
            context.getIpcRequestContext().setSessionId(response.session.id)
            return response
          },
          list: async ({ body }) => sessionManager.listSessions(body),
          get: async ({ body: { id } }) => ({
            session: await sessionManager.getSession(id),
          }),
          connect: async ({ body: { id } }) => ({
            session: await sessionManager.connectSession(id),
          }),
          history: async ({ body }) => sessionManager.getHistory(body),
          changes: async ({ body: { id } }) => sessionManager.getChanges(id),
          composerSuggestions: async ({ body }) => sessionManager.getComposerSuggestions(body),
          draftSuggestions: async ({ body }) => sessionManager.getDraftSuggestions(body),
          launchPreview: async ({ body }) => sessionManager.getLaunchPreview(body),
          launchLease: {
            release: async ({ body }) => sessionManager.releaseLaunchLease(body),
          },
          subpackages: async ({ body }) => sessionManager.getSubpackages(body),
          diagnostics: async ({ body: { id } }) => sessionManager.getDiagnostics(id),
          worktree: {
            get: async ({ body: { id } }) => sessionManager.getWorktree(id),
          },
          workforce: {
            get: async ({ body: { id } }) => sessionManager.getWorkforce(id),
          },
          shutdown: async ({ body: { id } }) => ({
            id,
            success: await sessionManager.shutdownSession(id),
          }),
          cancel: async ({ body: { id } }) => sessionManager.cancelSessionTurn(id),
          steer: async ({ body: { id, prompt } }) => sessionManager.steerSession(id, prompt),
          send: async ({ body: { id, message } }) => {
            await sessionManager.sendMessage(id, message as SendSessionMessageRequest["message"])
            return { accepted: true as const }
          },
          configOption: {
            set: async ({ body }) => ({
              session: await sessionManager.setSessionConfigOption(body),
            }),
          },
          model: {
            set: async ({ body }) => ({
              session: await sessionManager.setSessionModel(body),
            }),
          },
          complete: async ({ body: { id } }) => ({
            item: await sessionManager.completeSession(id),
          }),
          declareInitiative: async ({ body: { id, title } }) => ({
            session: await sessionManager.declareInitiative(id, title),
          }),
          reportBlocker: async ({ body: { id, reason, scope, headline } }) => ({
            session: await sessionManager.reportBlocker(id, reason, { scope, headline }),
          }),
          reportTurnEnded: async ({ body: { id, scope, headline } }) => ({
            session: await sessionManager.reportTurnEnded(id, { scope, headline }),
          }),
          resolveToken: async ({ body: { token } }) => {
            const id = await sessionManager.resolveSessionIdByToken(token)
            context.getIpcRequestContext().setSessionId(id)
            return {
              id,
            }
          },
          messageEvents: async function* (ctx) {
            const { query } = ctx
            yield* subscribeSessionMessages(query.id, ctx.request.signal)
          },
        },
      },
    }
  },
})

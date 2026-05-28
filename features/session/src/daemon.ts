import { definePlugin } from "@goddard-ai/daemon-plugin"
import type { SendSessionMessageRequest, SessionMessageEvent } from "@goddard-ai/schema/daemon"
import type { DaemonSessionId } from "@goddard-ai/schema/id"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import { createSessionEventEmitter } from "./daemon/events.ts"
import { createSessionManager } from "./daemon/manager.ts"

export { resolveAgentProcessSpec } from "./daemon/agent-process.ts"
export { injectSystemPrompt } from "./daemon/manager.ts"
export type { LoadSessionParams, NewSessionParams, SessionLaunchParams } from "./daemon/manager.ts"
export {
  type SessionEventEmitter,
  type SessionEvents,
  type SessionWorktreeLifecycleState,
} from "./daemon/events.ts"

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

    const { close, ...sessionMethods } = sessionManager

    return {
      provides: {
        session: {
          ...sessionMethods,
          events,
        },
      },
      close,
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

import { definePlugin } from "@goddard-ai/daemon-plugin"
import type { SendSessionMessageRequest, SessionMessageEvent } from "@goddard-ai/schema/daemon"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import { createSessionEventEmitter, type SessionEventEmitter } from "./daemon/events.ts"
import { createSessionManager, type SessionManager } from "./daemon/manager.ts"

export { injectSystemPrompt, resolveAgentProcessSpec } from "./daemon/manager.ts"
export { type SessionEventEmitter, type SessionEvents } from "./daemon/events.ts"

/** First-class session methods exposed to other daemon feature plugins. */
type SessionExtension = {
  create: SessionManager["newSession"] extends (input: { request: infer TRequest }) => unknown
    ? (request: TRequest) => ReturnType<SessionManager["newSession"]>
    : never
  list: SessionManager["listSessions"]
  connect: SessionManager["connectSession"]
  get: SessionManager["getSession"]
  history: SessionManager["getHistory"]
  changes: SessionManager["getChanges"]
  composerSuggestions: SessionManager["getComposerSuggestions"]
  draftSuggestions: SessionManager["getDraftSuggestions"]
  launchPreview: SessionManager["getLaunchPreview"]
  subpackages: SessionManager["getSubpackages"]
  diagnostics: SessionManager["getDiagnostics"]
  worktree: SessionManager["getWorktree"]
  mountReviewSession: SessionManager["mountReviewSession"]
  runReviewSession: SessionManager["runReviewSession"]
  unmountReviewSession: SessionManager["unmountReviewSession"]
  workforce: SessionManager["getWorkforce"]
  shutdown: SessionManager["shutdownSession"]
  prompt: SessionManager["promptSession"]
  cancel: SessionManager["cancelSessionTurn"]
  steer: SessionManager["steerSession"]
  sendMessage: SessionManager["sendMessage"]
  complete: SessionManager["completeSession"]
  declareInitiative: SessionManager["declareInitiative"]
  reportBlocker: SessionManager["reportBlocker"]
  reportTurnEnded: SessionManager["reportTurnEnded"]
  recordTurnAttentionActivity: SessionManager["recordTurnAttentionActivity"]
  resolveTokenScope: SessionManager["resolveTokenScope"]
  allowPullRequest: SessionManager["allowPullRequest"]
  subscriberConnected: SessionManager["sessionSubscriberConnected"]
  subscriberDisconnected: SessionManager["sessionSubscriberDisconnected"]
  resolveToken: SessionManager["resolveSessionIdByToken"]
  events: SessionEventEmitter
}

export const sessionPlugin = definePlugin({
  name: "session",
  ipcRoutes: sessionIpcRoutes,
  setup(context) {
    const events = createSessionEventEmitter()
    const messageListeners = new Set<(event: SessionMessageEvent) => void>()
    let sessionManager: SessionManager | undefined

    function getSessionManager() {
      sessionManager ??= createSessionManager({
        daemonUrl: context.daemonRuntime.getDaemonUrl(),
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
      return sessionManager
    }

    async function* subscribeSessionMessages(id: `ses_${string}`, signal: AbortSignal) {
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

      await session.subscriberConnected(id)
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
        await session.subscriberDisconnected(id)
      }
    }

    const session = {
      create: (request) => getSessionManager().newSession({ request }),
      list: (params) => getSessionManager().listSessions(params),
      connect: (id) => getSessionManager().connectSession(id),
      get: (id) => getSessionManager().getSession(id),
      history: (params) => getSessionManager().getHistory(params),
      changes: (id) => getSessionManager().getChanges(id),
      composerSuggestions: (params) => getSessionManager().getComposerSuggestions(params),
      draftSuggestions: (params) => getSessionManager().getDraftSuggestions(params),
      launchPreview: (params) => getSessionManager().getLaunchPreview(params),
      subpackages: (params) => getSessionManager().getSubpackages(params),
      diagnostics: (id) => getSessionManager().getDiagnostics(id),
      worktree: (id) => getSessionManager().getWorktree(id),
      mountReviewSession: (id) => getSessionManager().mountReviewSession(id),
      runReviewSession: (id) => getSessionManager().runReviewSession(id),
      unmountReviewSession: (id) => getSessionManager().unmountReviewSession(id),
      workforce: (id) => getSessionManager().getWorkforce(id),
      shutdown: (id) => getSessionManager().shutdownSession(id),
      prompt: (id, prompt) => getSessionManager().promptSession(id, prompt),
      cancel: (id) => getSessionManager().cancelSessionTurn(id),
      steer: (id, prompt) => getSessionManager().steerSession(id, prompt),
      sendMessage: (id, message) => getSessionManager().sendMessage(id, message),
      complete: (id) => getSessionManager().completeSession(id),
      declareInitiative: (id, title) => getSessionManager().declareInitiative(id, title),
      reportBlocker: (id, reason, metadata) =>
        getSessionManager().reportBlocker(id, reason, metadata),
      reportTurnEnded: (id, metadata) => getSessionManager().reportTurnEnded(id, metadata),
      recordTurnAttentionActivity: (id, metadata) =>
        getSessionManager().recordTurnAttentionActivity(id, metadata),
      resolveTokenScope: (token) => getSessionManager().resolveTokenScope(token),
      allowPullRequest: (id, prNumber) => getSessionManager().allowPullRequest(id, prNumber),
      subscriberConnected: (id) => getSessionManager().sessionSubscriberConnected(id),
      subscriberDisconnected: (id) => getSessionManager().sessionSubscriberDisconnected(id),
      resolveToken: (token) => getSessionManager().resolveSessionIdByToken(token),
      events,
    } satisfies SessionExtension

    return {
      provides: {
        session,
      },
      close: async () => {
        await sessionManager?.close()
      },
      ipcHandlers: {
        session: {
          create: async ({ body }) => {
            const response = {
              session: await session.create(body),
            }
            context.getIpcRequestContext().setSessionId(response.session.id)
            return response
          },
          list: async ({ body }) => session.list(body),
          get: async ({ body: { id } }) => ({
            session: await session.get(id),
          }),
          connect: async ({ body: { id } }) => ({
            session: await session.connect(id),
          }),
          history: async ({ body }) => session.history(body),
          changes: async ({ body: { id } }) => session.changes(id),
          composerSuggestions: async ({ body }) => session.composerSuggestions(body),
          draftSuggestions: async ({ body }) => session.draftSuggestions(body),
          launchPreview: async ({ body }) => session.launchPreview(body),
          subpackages: async ({ body }) => session.subpackages(body),
          diagnostics: async ({ body: { id } }) => session.diagnostics(id),
          worktree: {
            get: async ({ body: { id } }) => session.worktree(id),
          },
          reviewSession: {
            mount: async ({ body: { id } }) => session.mountReviewSession(id),
            run: async ({ body: { id } }) => session.runReviewSession(id),
            unmount: async ({ body: { id } }) => session.unmountReviewSession(id),
          },
          workforce: {
            get: async ({ body: { id } }) => session.workforce(id),
          },
          shutdown: async ({ body: { id } }) => ({
            id,
            success: await session.shutdown(id),
          }),
          cancel: async ({ body: { id } }) => session.cancel(id),
          steer: async ({ body: { id, prompt } }) => session.steer(id, prompt),
          send: async ({ body: { id, message } }) => {
            await session.sendMessage(id, message as SendSessionMessageRequest["message"])
            return { accepted: true as const }
          },
          complete: async ({ body: { id } }) => ({
            item: await session.complete(id),
          }),
          declareInitiative: async ({ body: { id, title } }) => ({
            session: await session.declareInitiative(id, title),
          }),
          reportBlocker: async ({ body: { id, reason, scope, headline } }) => ({
            session: await session.reportBlocker(id, reason, { scope, headline }),
          }),
          reportTurnEnded: async ({ body: { id, scope, headline } }) => ({
            session: await session.reportTurnEnded(id, { scope, headline }),
          }),
          resolveToken: async ({ body: { token } }) => {
            const id = await session.resolveToken(token)
            context.getIpcRequestContext().setSessionId(id)
            return {
              id,
            }
          },
          messageEvents: async function* (ctx) {
            const signal = (ctx as unknown as { readonly signal: AbortSignal }).signal
            const { query } = ctx
            yield* subscribeSessionMessages(query.id, signal)
          },
        },
      },
    }
  },
})

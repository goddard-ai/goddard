import { definePlugin } from "@goddard-ai/daemon-plugin"
import type { SendSessionMessageRequest } from "@goddard-ai/schema/daemon"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import { createSessionEventEmitter, type SessionEventEmitter } from "./daemon/events.ts"
import type { SessionManager } from "./daemon/manager.ts"

export {
  createSessionManager,
  injectSystemPrompt,
  resolveAgentProcessSpec,
  type SessionManager,
} from "./daemon/manager.ts"
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
    const session = {
      create: (request) => context.sessionManager.newSession({ request }),
      list: (params) => context.sessionManager.listSessions(params),
      connect: (id) => context.sessionManager.connectSession(id),
      get: (id) => context.sessionManager.getSession(id),
      history: (params) => context.sessionManager.getHistory(params),
      changes: (id) => context.sessionManager.getChanges(id),
      composerSuggestions: (params) => context.sessionManager.getComposerSuggestions(params),
      draftSuggestions: (params) => context.sessionManager.getDraftSuggestions(params),
      launchPreview: (params) => context.sessionManager.getLaunchPreview(params),
      subpackages: (params) => context.sessionManager.getSubpackages(params),
      diagnostics: (id) => context.sessionManager.getDiagnostics(id),
      worktree: (id) => context.sessionManager.getWorktree(id),
      mountReviewSession: (id) => context.sessionManager.mountReviewSession(id),
      runReviewSession: (id) => context.sessionManager.runReviewSession(id),
      unmountReviewSession: (id) => context.sessionManager.unmountReviewSession(id),
      workforce: (id) => context.sessionManager.getWorkforce(id),
      shutdown: (id) => context.sessionManager.shutdownSession(id),
      cancel: (id) => context.sessionManager.cancelSessionTurn(id),
      steer: (id, prompt) => context.sessionManager.steerSession(id, prompt),
      sendMessage: (id, message) => context.sessionManager.sendMessage(id, message),
      complete: (id) => context.sessionManager.completeSession(id),
      declareInitiative: (id, title) => context.sessionManager.declareInitiative(id, title),
      reportBlocker: (id, reason, metadata) =>
        context.sessionManager.reportBlocker(id, reason, metadata),
      reportTurnEnded: (id, metadata) => context.sessionManager.reportTurnEnded(id, metadata),
      recordTurnAttentionActivity: (id, metadata) =>
        context.sessionManager.recordTurnAttentionActivity(id, metadata),
      resolveTokenScope: (token) => context.sessionManager.resolveTokenScope(token),
      allowPullRequest: (id, prNumber) => context.sessionManager.allowPullRequest(id, prNumber),
      subscriberConnected: (id) => context.sessionManager.sessionSubscriberConnected(id),
      subscriberDisconnected: (id) => context.sessionManager.sessionSubscriberDisconnected(id),
      resolveToken: (token) => context.sessionManager.resolveSessionIdByToken(token),
      events,
    } satisfies SessionExtension

    return {
      provides: {
        session,
      },
      routeHandlers: {
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
          messageEvents: async function* () {},
        },
      },
    }
  },
})

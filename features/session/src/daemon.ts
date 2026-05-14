import { definePlugin, defineSetupContext } from "@goddard-ai/daemon-plugin"
import type { Handlers } from "@goddard-ai/ipc"
import type { SendSessionMessageRequest } from "@goddard-ai/schema/daemon"
import type { DaemonSessionId } from "@goddard-ai/schema/id"

import { sessionIpcSchema } from "./daemon-ipc.ts"
import { createSessionEventEmitter, type SessionEventEmitter } from "./daemon/events.ts"
import type { SessionManager } from "./daemon/manager.ts"

export {
  createSessionManager,
  injectSystemPrompt,
  resolveAgentProcessSpec,
  type SessionManager,
} from "./daemon/manager.ts"
export { type SessionEventEmitter, type SessionEvents } from "./daemon/events.ts"

/** Daemon-owned runtime objects the session feature needs to bind IPC handlers. */
type SessionSetupContext = {
  sessionManager: SessionManager
  setRequestSessionId: (id: DaemonSessionId) => void
}

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
  subscriberConnected: SessionManager["sessionSubscriberConnected"]
  subscriberDisconnected: SessionManager["sessionSubscriberDisconnected"]
  resolveToken: SessionManager["resolveSessionIdByToken"]
  events: SessionEventEmitter
}

export const sessionPlugin = definePlugin({
  name: "session",
  ipc: sessionIpcSchema,
  setupContext: defineSetupContext<SessionSetupContext>(),
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
      subscriberConnected: (id) => context.sessionManager.sessionSubscriberConnected(id),
      subscriberDisconnected: (id) => context.sessionManager.sessionSubscriberDisconnected(id),
      resolveToken: (token) => context.sessionManager.resolveSessionIdByToken(token),
      events,
    } satisfies SessionExtension

    return {
      provides: {
        session,
      },
      requestHandlers: {
        "session.create": async (payload) => {
          const response = {
            session: await session.create(payload),
          }
          context.setRequestSessionId(response.session.id)
          return response
        },
        "session.list": async (payload) => session.list(payload),
        "session.get": async ({ id }) => ({
          session: await session.get(id),
        }),
        "session.connect": async ({ id }) => ({
          session: await session.connect(id),
        }),
        "session.history": async (payload) => session.history(payload),
        "session.changes": async ({ id }) => session.changes(id),
        "session.composerSuggestions": async (payload) => session.composerSuggestions(payload),
        "session.draftSuggestions": async (payload) => session.draftSuggestions(payload),
        "session.launchPreview": async (payload) => session.launchPreview(payload),
        "session.subpackages": async (payload) => session.subpackages(payload),
        "session.diagnostics": async ({ id }) => session.diagnostics(id),
        "session.worktree.get": async ({ id }) => session.worktree(id),
        "session.reviewSession.mount": async ({ id }) => session.mountReviewSession(id),
        "session.reviewSession.run": async ({ id }) => session.runReviewSession(id),
        "session.reviewSession.unmount": async ({ id }) => session.unmountReviewSession(id),
        "session.workforce.get": async ({ id }) => session.workforce(id),
        "session.shutdown": async ({ id }) => ({
          id,
          success: await session.shutdown(id),
        }),
        "session.cancel": async ({ id }) => session.cancel(id),
        "session.steer": async ({ id, prompt }) => session.steer(id, prompt),
        "session.send": async ({ id, message }) => {
          await session.sendMessage(id, message as SendSessionMessageRequest["message"])
          return { accepted: true as const }
        },
        "session.complete": async ({ id }) => ({
          item: await session.complete(id),
        }),
        "session.declareInitiative": async ({ id, title }) => ({
          session: await session.declareInitiative(id, title),
        }),
        "session.reportBlocker": async ({ id, reason, scope, headline }) => ({
          session: await session.reportBlocker(id, reason, { scope, headline }),
        }),
        "session.reportTurnEnded": async ({ id, scope, headline }) => ({
          session: await session.reportTurnEnded(id, { scope, headline }),
        }),
        "session.resolveToken": async ({ token }) => {
          const id = await session.resolveToken(token)
          context.setRequestSessionId(id)
          return {
            id,
          }
        },
      } satisfies Handlers<typeof sessionIpcSchema>,
    }
  },
})

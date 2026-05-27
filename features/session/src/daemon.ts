import { definePlugin, event, type DbContext } from "@goddard-ai/daemon-plugin"
import { kind } from "kindstore"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import {
  createSessionManager,
  type SessionActivatedEvent,
  type SessionAttentionEvent,
  type SessionBlockedEvent,
  type SessionIdEvent,
  type SessionLaunchFailedEvent,
  type SessionLaunchFinishedEvent,
  type SessionPersistedEvent,
  type SessionStoppingEvent,
  type SessionWorktreePreparedEvent,
} from "./daemon/manager.ts"
import {
  DaemonSession,
  DaemonSessionDiagnostics,
  DaemonSessionTurn,
  DaemonSessionTurnDraft,
  DaemonWorktree,
  SessionsConfig,
  SessionTitlesConfig,
  StaticSessionParams,
  SubpackagesConfig,
  WorktreesConfig,
  type SendSessionMessageRequest,
  type SessionId,
  type SessionMessageEvent,
} from "./schema.ts"

export { resolveAgentProcessSpec } from "./daemon/agent-process.ts"
export { injectSystemPrompt } from "./daemon/manager.ts"
export type {
  LoadSessionParams,
  NewSessionParams,
  SessionEventEmitter,
  SessionLaunchParams,
  SessionWorktreeLifecycleState,
} from "./daemon/manager.ts"

const sessionDb = {
  sessions: kind("ses", DaemonSession)
    .createdAt()
    .updatedAt()
    .index("acpSessionId")
    .index("repository")
    .index("token")
    .multi("repository_prNumber", {
      repository: "asc",
      prNumber: "asc",
    })
    .multi("updatedAt_id", {
      updatedAt: "desc",
      id: "desc",
    })
    .multi("completedHidden_updatedAt_id", {
      completedHidden: "asc",
      updatedAt: "desc",
      id: "desc",
    }),

  sessionTurns: kind("trn", DaemonSessionTurn)
    .index("sessionId", { type: "text" })
    .index("sequence", { type: "integer" })
    .multi("sessionId_sequence", {
      sessionId: "asc",
      sequence: "desc",
    }),

  sessionTurnDrafts: kind("drf", DaemonSessionTurnDraft)
    .index("sessionId", { type: "text" })
    .index("sequence", { type: "integer" })
    .multi("sessionId_sequence", {
      sessionId: "asc",
      sequence: "desc",
    }),

  sessionDiagnostics: kind("dgn", DaemonSessionDiagnostics).index("sessionId", {
    type: "text",
  }),

  worktrees: kind("wt", DaemonWorktree).index("sessionId", { type: "text" }),
}

export const sessionPlugin = definePlugin({
  name: "session",
  config: {
    session: {
      schema: StaticSessionParams,
      scopes: ["user", "project"],
      resolve: ({ project, user }) => project ?? user,
    },
    sessions: {
      schema: SessionsConfig,
      scopes: ["user", "project"],
    },
    worktrees: {
      schema: WorktreesConfig,
      scopes: ["user", "project"],
    },
    sessionTitles: {
      schema: SessionTitlesConfig,
      scopes: ["user", "project"],
    },
    subpackages: {
      schema: SubpackagesConfig,
      scopes: ["user", "project"],
    },
  },
  db: sessionDb,
  events: {
    "session.worktree.prepared": event<SessionWorktreePreparedEvent>(),
    "session.persisted": event<SessionPersistedEvent>(),
    "session.activated": event<SessionActivatedEvent>(),
    "session.launch.finished": event<SessionLaunchFinishedEvent>(),
    "session.launch.failed": event<SessionLaunchFailedEvent>(),
    "session.stopping": event<SessionStoppingEvent>(),
    "session.blocked": event<SessionBlockedEvent>(),
    "session.turn.ended": event<SessionAttentionEvent>(),
    "session.replied": event<SessionIdEvent>(),
    "session.completed": event<SessionIdEvent>(),
  },
  ipcRoutes: sessionIpcRoutes,
  setup(context) {
    const messageListeners = new Set<(event: SessionMessageEvent) => void>()
    const sessionManager = createSessionManager({
      db: context.db,
      getDaemonUrl: context.daemonRuntime.getDaemonUrl,
      createAgentEnvironment: context.daemonRuntime.createAgentEnvironment,
      configProvider: context.configProvider,
      log: context.log,
      registryService: context.registryService,
      sessionContext: context.sessionContext,
      events: context.events,
      idleSessionShutdownTimeoutMs: context.daemonRuntime.idleSessionShutdownTimeoutMs,
      emitMessage(id, message) {
        for (const listener of messageListeners) {
          listener({ id, message })
        }
      },
    })

    async function* subscribeSessionMessages(id: SessionId, signal: AbortSignal) {
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
          newSession: sessionManager.newSession,
          promptSession: sessionManager.promptSession,
          shutdownSession: sessionManager.shutdownSession,
          recordTurnAttentionActivity: sessionManager.recordTurnAttentionActivity,
          recordSessionResult: sessionManager.recordSessionResult,
          resolveTokenScope: sessionManager.resolveTokenScope,
          allowPullRequest: sessionManager.allowPullRequest,
          completeSession: sessionManager.completeSession,
          getSession: sessionManager.getSession,
          getWorktree: sessionManager.getWorktree,
          requireWorktree: sessionManager.requireWorktree,
          listWorktrees: sessionManager.listWorktrees,
          findWorktreeByDir: sessionManager.findWorktreeByDir,
          isActive: sessionManager.isActive,
          emitDiagnostic: sessionManager.emitDiagnostic,
        },
      },
      close: sessionManager.close,
      ipcHandlers: {
        session: {
          create: async ({ body }) => {
            const response = {
              session: await sessionManager.newSession({ request: body }),
            }
            context.ipc.requestContext.setSessionId(response.session.id)
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
          promptHistory: async ({ body }) => sessionManager.getPromptHistory(body),
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
            session: await sessionManager.completeSession(id),
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
            context.ipc.requestContext.setSessionId(id)
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

export type SessionDb = DbContext<typeof sessionDb>

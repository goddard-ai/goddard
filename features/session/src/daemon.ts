import { definePlugin, type DbContext } from "@goddard-ai/daemon-plugin"
import { managedAgentPlugin } from "@goddard-ai/managed-agent/daemon"
import { kind } from "kindstore"
import { isObject } from "radashi"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import { createSessionManager } from "./daemon/manager.ts"
import { sessionEvents } from "./events.ts"
import {
  DaemonLaunchWorktree,
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
  type SessionId,
  type SessionTurnMessage,
} from "./schema.ts"

export { resolveUnmanagedAgentProcessSpec } from "./daemon/agent-process.ts"
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
    .index("acpSessionId")
    .index("repository")
    .index("token")
    .multi("repository_prNumber", {
      repository: "asc",
      prNumber: "asc",
    })
    .multi("lastSessionActivityAt_id", {
      lastSessionActivityAt: "desc",
      id: "desc",
    })
    .multi("completedHidden_lastSessionActivityAt_id", {
      completedHidden: "asc",
      lastSessionActivityAt: "desc",
      id: "desc",
    })
    .migrate(3, {
      1: (value, context) => {
        const { updatedAt, ...session } = value
        return {
          ...session,
          lastSessionActivityAt: typeof updatedAt === "number" ? updatedAt : context.now,
        }
      },
      2: (value) => {
        const { models: _models, ...session } = value
        return session
      },
    }),

  sessionTurns: kind("trn", DaemonSessionTurn)
    .index("sessionId", { type: "text" })
    .index("sequence", { type: "integer" })
    .multi(
      "sessionId_sequence",
      {
        sessionId: "asc",
        sequence: "desc",
      },
      { unique: true },
    )
    .migrate(2, {
      1: (turn) => ({
        ...turn,
        messages: sanitizeSessionTurnMessages(turn.messages),
      }),
    }),

  sessionTurnDrafts: kind("drf", DaemonSessionTurnDraft)
    .index("sessionId", { type: "text", unique: true })
    .index("sequence", { type: "integer" })
    .multi("sessionId_sequence", {
      sessionId: "asc",
      sequence: "desc",
    })
    .migrate(2, {
      1: (draft) => ({
        ...draft,
        messages: sanitizeSessionTurnMessages(draft.messages),
      }),
    }),

  sessionDiagnostics: kind("dgn", DaemonSessionDiagnostics).index("sessionId", {
    type: "text",
  }),

  worktrees: kind("wt", DaemonWorktree).index("sessionId", { type: "text" }),

  launchWorktrees: kind("lwt", DaemonLaunchWorktree).index("key", { type: "text" }),
}

type SessionTurnRetentionRecord = {
  id: string
  completedAt: string | null
  messages: readonly unknown[]
  startedAt: string
}

type SessionTurnDraftRetentionRecord = {
  id: string
  sequence: number
  startedAt: string
  updatedAt: string
  messages: readonly unknown[]
}

function sanitizeSessionTurnMessages(messages: unknown) {
  if (!Array.isArray(messages)) {
    return []
  }

  return messages.flatMap((message): SessionTurnMessage[] => {
    if (!isObject(message)) {
      return []
    }

    const record = message as Record<string, unknown>

    if (
      !Number.isInteger(record.sequence) ||
      !Number.isInteger(record.sequenceStart) ||
      !isObject(record.message)
    ) {
      return []
    }

    const sequence = record.sequence as number
    const sequenceStart = record.sequenceStart as number
    const payload = record.message as SessionTurnMessage["message"]

    return [
      {
        sequence,
        sequenceStart,
        message: payload,
      } satisfies SessionTurnMessage,
    ]
  })
}

function compareSessionTurnRetention(
  left: SessionTurnRetentionRecord,
  right: SessionTurnRetentionRecord,
) {
  return (
    compareNullableTextDesc(left.completedAt, right.completedAt) ||
    compareNumberDesc(left.messages.length, right.messages.length) ||
    compareTextDesc(left.startedAt, right.startedAt) ||
    left.id.localeCompare(right.id)
  )
}

function compareSessionTurnDraftRetention(
  left: SessionTurnDraftRetentionRecord,
  right: SessionTurnDraftRetentionRecord,
) {
  return (
    compareTextDesc(left.updatedAt, right.updatedAt) ||
    compareNumberDesc(left.sequence, right.sequence) ||
    compareNumberDesc(left.messages.length, right.messages.length) ||
    compareTextDesc(left.startedAt, right.startedAt) ||
    left.id.localeCompare(right.id)
  )
}

function compareNullableTextDesc(left: string | null, right: string | null) {
  if (left === right) {
    return 0
  }
  if (left === null) {
    return 1
  }
  if (right === null) {
    return -1
  }
  return compareTextDesc(left, right)
}

function compareTextDesc(left: string, right: string) {
  return right.localeCompare(left)
}

function compareNumberDesc(left: number, right: number) {
  return right - left
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
  db: {
    schema: sessionDb,
    migrate(m) {
      m.prepareConstraints("2026-06-dedupe-session-turns", ({ db }) => {
        const turns = db.sessionTurns.findMany()
        const retainedTurns = new Map<string, (typeof turns)[number]>()

        for (const turn of turns) {
          const key = `${turn.sessionId}\0${turn.sequence}`
          const retained = retainedTurns.get(key)
          if (
            !retained ||
            compareSessionTurnRetention(
              turn as SessionTurnRetentionRecord,
              retained as SessionTurnRetentionRecord,
            ) < 0
          ) {
            retainedTurns.set(key, turn)
          }
        }

        for (const turn of turns) {
          if (retainedTurns.get(`${turn.sessionId}\0${turn.sequence}`)?.id !== turn.id) {
            db.sessionTurns.delete(turn.id)
          }
        }
      })

      m.prepareConstraints("2026-06-dedupe-session-turn-drafts", ({ db }) => {
        const drafts = db.sessionTurnDrafts.findMany()
        const retainedDrafts = new Map<string, (typeof drafts)[number]>()

        for (const draft of drafts) {
          const retained = retainedDrafts.get(draft.sessionId)
          if (
            !retained ||
            compareSessionTurnDraftRetention(
              draft as SessionTurnDraftRetentionRecord,
              retained as SessionTurnDraftRetentionRecord,
            ) < 0
          ) {
            retainedDrafts.set(draft.sessionId, draft)
          }
        }

        for (const draft of drafts) {
          if (retainedDrafts.get(draft.sessionId)?.id !== draft.id) {
            db.sessionTurnDrafts.delete(draft.id)
          }
        }
      })
    },
  },
  events: sessionEvents,
  ipcRoutes: sessionIpcRoutes,
  consumes: [managedAgentPlugin],
  setup({ configProvider, daemonRuntime, db, events, ipc, log, managedAgent, sessionContext }) {
    const streamDebug = log.createDebug("session.stream")
    const sessionManager = createSessionManager({
      db,
      getDaemonUrl: daemonRuntime.getDaemonUrl,
      createAgentEnvironment: daemonRuntime.createAgentEnvironment,
      configProvider,
      log,
      managedAgent,
      sessionContext,
      events,
      idleSessionShutdownTimeoutMs: daemonRuntime.idleSessionShutdownTimeoutMs,
    })

    events.onSubscription(async (subscription) => {
      const id = readSessionMessageSubscriptionId(subscription.filter)
      if (!id) {
        return
      }

      if (subscription.state === "started") {
        await sessionManager.sessionSubscriberConnected(id)
        streamDebug("session.stream.message_subscriber_attached", { sessionId: id })
        return
      }

      await sessionManager.sessionSubscriberDisconnected(id)
      streamDebug("session.stream.message_subscriber_detached", { sessionId: id })
    })

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
            ipc.requestContext.setSessionId(response.session.id)
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
          launchWorktree: {
            prepare: async ({ body }) => sessionManager.prepareLaunchWorktree(body),
            release: async ({ body }) => sessionManager.releaseLaunchWorktree(body),
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
          popQueuedPrompt: async ({ body: { id } }) => sessionManager.popQueuedPrompt(id),
          send: async ({ body: { id, message } }) => {
            await sessionManager.sendMessage(
              id,
              message as Parameters<typeof sessionManager.sendMessage>[1],
            )
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
            ipc.requestContext.setSessionId(id)
            return {
              id,
            }
          },
        },
      },
    }
  },
})

function readSessionMessageSubscriptionId(filter: {
  readonly names?: readonly string[]
  readonly where?: readonly { readonly path: string; readonly equals: unknown }[]
}) {
  if (!filter.names?.includes("session.message")) {
    return null
  }

  const id = filter.where?.find((condition) => condition.path === "id")?.equals
  return typeof id === "string" && id.startsWith("ses_") ? (id as SessionId) : null
}

export type SessionDb = DbContext<typeof sessionDb>

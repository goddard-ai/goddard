import { resolveDefaultAgent } from "@goddard-ai/config/node"
import { definePlugin } from "@goddard-ai/daemon-plugin"
import { IpcClientError } from "@goddard-ai/ipc"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { kind } from "kindstore"

import { workforceIpcRoutes } from "./daemon-ipc.ts"
import {
  discoverWorkforceInitCandidates,
  initializeWorkforce,
  resolveRepositoryRoot,
} from "./daemon/config.ts"
import { WorkforceActorContext, WorkforceDispatchContext } from "./daemon/context.ts"
import { createWorkforceManager } from "./daemon/manager.ts"
import { normalizeWorkforceRootDir } from "./daemon/paths.ts"
import { workforceEvents } from "./events.ts"
import {
  DaemonWorkforce,
  WorkforceRootConfig,
  type WorkforceRootConfig as WorkforceRootConfigType,
} from "./schema.ts"

export const workforcePlugin = definePlugin({
  name: "workforce",
  consumes: [sessionPlugin],
  config: {
    workforce: {
      schema: WorkforceRootConfig,
      scopes: ["user", "project"],
    },
  },
  db: {
    schema: {
      workforces: kind("wf", DaemonWorkforce).index("sessionId", { type: "text" }),
    },
  },
  events: workforceEvents,
  ipcRoutes: workforceIpcRoutes,
  logContext: {
    read: () => ({
      workforceActor: WorkforceActorContext.get(),
      workforceDispatch: WorkforceDispatchContext.get(),
    }),
  },
  setup({ configProvider, db, events, ipc, log, session }) {
    const workforce = createWorkforceManager({
      log,
      session,
      attachSession: ({ sessionId, rootDir, agentId, requestId }) => {
        const nextWorkforce = {
          sessionId,
          rootDir,
          agentId,
          requestId,
        }
        const existingRecord =
          db.workforces.first({
            where: { sessionId },
          }) ?? null
        if (existingRecord) {
          db.workforces.put(existingRecord.id, nextWorkforce)
        } else {
          db.workforces.create(nextWorkforce)
        }
      },
      publishEvent: (payload) => {
        void events.emit("workforce.ledger.event", payload)
      },
    })

    async function resolveWorkforceActor(token: string | undefined, requestedRootDir: string) {
      if (!token) {
        return {
          sessionId: null,
          rootDir: null,
          agentId: null,
          requestId: null,
        }
      }

      const tokenScope = await session.resolveTokenScope(token)
      if (!tokenScope) {
        throw new IpcClientError("Invalid session token")
      }

      ipc.requestContext.setSessionId(tokenScope.sessionId)

      const workforceRecord =
        db.workforces.first({
          where: { sessionId: tokenScope.sessionId },
        }) ?? null
      if (!workforceRecord || typeof workforceRecord.agentId !== "string") {
        throw new IpcClientError("Session is not attached to a workforce request")
      }

      if (typeof workforceRecord.rootDir !== "string") {
        throw new IpcClientError("Session is not attached to a workforce root")
      }

      const [sessionRootDir, normalizedRequestedRootDir] = await Promise.all([
        normalizeWorkforceRootDir(workforceRecord.rootDir),
        normalizeWorkforceRootDir(requestedRootDir),
      ])

      if (sessionRootDir !== normalizedRequestedRootDir) {
        throw new IpcClientError(
          `Session workforce root ${sessionRootDir} does not match requested root ${normalizedRequestedRootDir}`,
        )
      }

      return {
        sessionId: tokenScope.sessionId,
        rootDir: sessionRootDir,
        agentId: workforceRecord.agentId,
        requestId: typeof workforceRecord.requestId === "string" ? workforceRecord.requestId : null,
      }
    }

    function requireActorRequestId(actor: { readonly requestId: string | null }) {
      if (!actor.requestId) {
        throw new IpcClientError("Session is not attached to an active workforce request")
      }

      return actor.requestId
    }

    return {
      close: () => {
        workforce.close()
      },
      ipcHandlers: {
        session: {
          workforce: {
            get: async ({ body: { id } }) => {
              const sessionRecord = await session.getSession(id)
              return {
                id: sessionRecord.id,
                acpSessionId: sessionRecord.acpSessionId,
                workforce:
                  db.workforces.first({
                    where: { sessionId: id },
                  }) ?? null,
              }
            },
          },
        },
        workforce: {
          start: async ({ body: { rootDir } }) => ({
            workforce: await workforce.startWorkforce(rootDir),
          }),
          discoverCandidates: async ({ body: { rootDir } }) => {
            const repositoryRoot = await resolveRepositoryRoot(rootDir)
            return {
              rootDir: repositoryRoot,
              candidates: await discoverWorkforceInitCandidates(repositoryRoot),
            }
          },
          initialize: async ({ body: { rootDir, packageDirs } }) => {
            const repositoryRoot = await resolveRepositoryRoot(rootDir)
            const config = await configProvider
              .getRootConfig(repositoryRoot)
              .then((root) => root.config)
            return {
              initialized: await initializeWorkforce(repositoryRoot, packageDirs, {
                defaultAgent:
                  (config.workforce as WorkforceRootConfigType | undefined)?.defaultAgent ??
                  (await resolveDefaultAgent(config)),
              }),
            }
          },
          get: async ({ body: { rootDir } }) => ({
            workforce: await workforce.getWorkforce(rootDir),
          }),
          list: async () => ({
            workforces: await workforce.listWorkforces(),
          }),
          shutdown: async ({ body: { rootDir } }) => ({
            rootDir,
            success: await workforce.shutdownWorkforce(rootDir),
          }),
          request: async ({ body }) => {
            const actor = await resolveWorkforceActor(body.token, body.rootDir)
            return workforce.appendWorkforceEvent(
              actor.rootDir ?? body.rootDir,
              {
                type: "request",
                targetAgentId: body.targetAgentId,
                input: body.input,
                intent: body.intent,
              },
              actor,
            )
          },
          update: async ({ body }) => {
            const actor = await resolveWorkforceActor(body.token, body.rootDir)
            return workforce.appendWorkforceEvent(
              actor.rootDir ?? body.rootDir,
              {
                type: "update",
                requestId: body.requestId,
                input: body.input,
              },
              actor,
            )
          },
          cancel: async ({ body }) => {
            const actor = await resolveWorkforceActor(body.token, body.rootDir)
            return workforce.appendWorkforceEvent(
              actor.rootDir ?? body.rootDir,
              {
                type: "cancel",
                requestId: body.requestId,
                reason: body.reason ?? null,
              },
              actor,
            )
          },
          truncate: async ({ body }) => {
            const actor = await resolveWorkforceActor(body.token, body.rootDir)
            return workforce.appendWorkforceEvent(
              actor.rootDir ?? body.rootDir,
              {
                type: "truncate",
                agentId: body.agentId ?? null,
                reason: body.reason ?? null,
              },
              actor,
            )
          },
          respond: async ({ body }) => {
            const actor = await resolveWorkforceActor(body.token, body.rootDir)
            return workforce.appendWorkforceEvent(
              actor.rootDir ?? body.rootDir,
              {
                type: "respond",
                requestId: requireActorRequestId(actor),
                output: body.output,
              },
              actor,
            )
          },
          suspend: async ({ body }) => {
            const actor = await resolveWorkforceActor(body.token, body.rootDir)
            return workforce.appendWorkforceEvent(
              actor.rootDir ?? body.rootDir,
              {
                type: "suspend",
                requestId: requireActorRequestId(actor),
                reason: body.reason,
              },
              actor,
            )
          },
        },
      },
    }
  },
})

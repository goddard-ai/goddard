import { authBackendRoutes } from "@goddard-ai/auth/backend"
import { composeBackendRoutes } from "@goddard-ai/backend-plugin"
import { cloudSessionBackendRoutes } from "@goddard-ai/cloud-session/backend"
import {
  pullRequestBackendRoutes,
  pullRequestRemoteRepoEventHandler,
} from "@goddard-ai/pull-request/backend"
import {
  dispatchRemoteRepoEvent,
  remoteRepoBackendRoutes,
  type RemoteRepoEventHandler,
} from "@goddard-ai/remote-repo/backend"
import { GitHubWebhookInput, type RepoEvent } from "@goddard-ai/remote-repo/schema"
import { createClient } from "@libsql/client/web"
import { getErrorMessage } from "radashi"
import { createRouter } from "rouzer"

import { createCloudSessionId } from "../cloud-session.ts"
import { TursoBackendControlPlane } from "../db/persistence.ts"
import type { Env } from "../env.ts"
import { assertRepo, HttpError, type BackendControlPlane } from "./control-plane.ts"

const backendRoutes = composeBackendRoutes([
  authBackendRoutes,
  cloudSessionBackendRoutes,
  pullRequestBackendRoutes,
  remoteRepoBackendRoutes,
])

/** Test seams and runtime adapters injected into the backend router. */
type RouterDependencies = {
  createControlPlane?: (env: Env) => BackendControlPlane
  broadcastEvent?: (env: Env, event: RepoEvent) => Promise<void>
  handleUserStream?: (env: Env, githubUsername: string, request: Request) => Promise<Response>
  remoteRepoEventHandlers?: readonly RemoteRepoEventHandler[]
  handleCloudSession?: (
    env: Env,
    githubUsername: string,
    request: Request,
    options: CloudSessionHandlerOptions,
  ) => Promise<Response>
}

/** Forwarding details for a cloud-session request owned by the worker runtime. */
export type CloudSessionHandlerOptions = {
  sessionId: string
  pathname: string
  body?: unknown
}

/** Creates the backend HTTP router over the current control-plane implementation. */
export function createBackendRouter(dependencies: RouterDependencies = {}) {
  const createControlPlane = dependencies.createControlPlane ?? createTursoControlPlane
  const broadcastEvent = dependencies.broadcastEvent ?? noopBroadcast
  const handleUserStream = dependencies.handleUserStream ?? defaultHandleUserStream
  const remoteRepoEventHandlers = dependencies.remoteRepoEventHandlers ?? [
    pullRequestRemoteRepoEventHandler,
  ]
  const handleCloudSession = dependencies.handleCloudSession ?? defaultHandleCloudSession

  return createRouter<Env>({ debug: false }).use(backendRoutes, {
    auth: {
      device: {
        start: async (ctx) => {
          try {
            const controlPlane = createControlPlane(readEnv(ctx))
            return await controlPlane.startDeviceFlow(ctx.body)
          } catch (error) {
            return toErrorResponse(error)
          }
        },
        complete: async (ctx) => {
          try {
            const controlPlane = createControlPlane(readEnv(ctx))
            return await controlPlane.completeDeviceFlow(ctx.body)
          } catch (error) {
            return toErrorResponse(error)
          }
        },
      },
      session: {
        current: async (ctx) => {
          try {
            const controlPlane = createControlPlane(readEnv(ctx))
            const token = readBearerToken(ctx.headers.authorization)
            return await controlPlane.getSession(token)
          } catch (error) {
            return toErrorResponse(error)
          }
        },
      },
    },
    pullRequests: {
      create: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const token = readBearerToken(ctx.headers.authorization)
          const pr = await controlPlane.createPr(token, ctx.body, env)

          await broadcastEvent(env, {
            type: "pr.created",
            owner: pr.owner,
            repo: pr.repo,
            prNumber: pr.number,
            title: pr.title,
            author: pr.createdBy,
            createdAt: pr.createdAt,
          })

          return pr
        } catch (error) {
          return toErrorResponse(error)
        }
      },
      managed: async (ctx) => {
        try {
          const controlPlane = createControlPlane(readEnv(ctx))
          const token = readBearerToken(ctx.headers.authorization)
          const session = await controlPlane.getSession(token)
          const { owner, repo, prNumber } = ctx.query
          assertRepo(owner, repo)
          const managed = await controlPlane.isManagedPr(
            owner,
            repo,
            prNumber,
            session.githubUsername,
          )
          return { managed }
        } catch (error) {
          return toErrorResponse(error)
        }
      },
      comments: {
        create: async (ctx) => {
          try {
            const env = readEnv(ctx)
            const controlPlane = createControlPlane(env)
            const token = readBearerToken(ctx.headers.authorization)
            await controlPlane.replyToPr(token, ctx.body, env)
            return { success: true }
          } catch (error) {
            return toErrorResponse(error)
          }
        },
      },
    },
    webhooks: {
      github: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const input = GitHubWebhookInput.parse(await ctx.request.json())
          const event = await controlPlane.handleGitHubWebhook(input)
          await dispatchRemoteRepoEvent(event, remoteRepoEventHandlers)
          await broadcastEvent(env, event)
          return event
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    remoteRepo: {
      stream: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const token = readBearerToken(ctx.headers.authorization)
          const session = await controlPlane.getSession(token)

          return await handleUserStream(env, session.githubUsername, ctx.request)
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    cloudSessionCreateRoute: {
      POST: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const token = readBearerToken(ctx.headers.authorization)
          const session = await controlPlane.getSession(token)
          const sessionId = ctx.body.sessionId ?? createCloudSessionId()

          return await handleCloudSession(env, session.githubUsername, ctx.request, {
            sessionId,
            pathname: "/create",
            body: { ...ctx.body, sessionId },
          })
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    cloudSessionCreateByIdRoute: {
      POST: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const token = readBearerToken(ctx.headers.authorization)
          const session = await controlPlane.getSession(token)

          return await handleCloudSession(env, session.githubUsername, ctx.request, {
            sessionId: ctx.path.sessionId,
            pathname: "/create",
            body: { ...ctx.body, sessionId: ctx.path.sessionId },
          })
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    cloudSessionSyncRoute: {
      GET: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const token = readBearerToken(ctx.headers.authorization)
          const session = await controlPlane.getSession(token)

          return await handleCloudSession(env, session.githubUsername, ctx.request, {
            sessionId: ctx.path.sessionId,
            pathname: "/sync",
          })
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    cloudSessionCommandRoute: {
      POST: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const token = readBearerToken(ctx.headers.authorization)
          const session = await controlPlane.getSession(token)

          return await handleCloudSession(env, session.githubUsername, ctx.request, {
            sessionId: ctx.path.sessionId,
            pathname: "/commands",
            body: ctx.body,
          })
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
  })
}

/** Builds the default Turso-backed control-plane implementation for one request environment. */
function createTursoControlPlane(env: Env): BackendControlPlane {
  const client = createClient({
    url: env.TURSO_DB_URL,
    authToken: env.TURSO_DB_AUTH_TOKEN,
  })

  return new TursoBackendControlPlane(client as any)
}

/** Provides a safe default when the worker host does not supply event broadcasting. */
async function noopBroadcast(_env: Env, _event: RepoEvent): Promise<void> {
  // No-op: the caller (e.g. worker.js) should provide a real implementation.
}

/** Returns a clear placeholder response when server-sent events are not wired in. */
async function defaultHandleUserStream(
  _env: Env,
  _githubUsername: string,
  _request: Request,
): Promise<Response> {
  return new Response("SSE handler not configured", { status: 501 })
}

/** Returns a clear placeholder response when cloud-session coordination is not wired in. */
async function defaultHandleCloudSession(
  _env: Env,
  _githubUsername: string,
  _request: Request,
  _options: CloudSessionHandlerOptions,
) {
  return new Response("Cloud session handler not configured", { status: 501 })
}

/** Rehydrates the worker environment values used by the backend control plane. */
function readEnv(ctx: { env: <K extends keyof Env>(key: K) => Env[K] }): Env {
  return {
    TURSO_DB_URL: ctx.env("TURSO_DB_URL"),
    TURSO_DB_AUTH_TOKEN: ctx.env("TURSO_DB_AUTH_TOKEN"),
    GITHUB_APP_ID: ctx.env("GITHUB_APP_ID"),
    GITHUB_APP_PRIVATE_KEY: ctx.env("GITHUB_APP_PRIVATE_KEY"),
    USER_STREAM: ctx.env("USER_STREAM"),
    CLOUD_SESSION: ctx.env("CLOUD_SESSION"),
    GODDARD_BACKEND_TEST_MODE: ctx.env("GODDARD_BACKEND_TEST_MODE"),
  }
}

/** Extracts the bearer token expected by authenticated backend routes. */
function readBearerToken(header: string): string {
  if (!header || !header.startsWith("Bearer ")) {
    throw new HttpError(401, "Missing Bearer token")
  }

  return header.slice("Bearer ".length)
}

/** Converts thrown backend errors into consistent JSON HTTP responses. */
function toErrorResponse(error: unknown): Response {
  const statusCode = error instanceof HttpError ? error.statusCode : 500
  const message = getErrorMessage(error)
  return Response.json({ error: message }, { status: statusCode })
}

const router = createBackendRouter()

export default router

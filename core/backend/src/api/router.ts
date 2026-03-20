import type { RepoEvent } from "@goddard-ai/schema/backend"
import * as routes from "@goddard-ai/schema/backend/routes"
import { createClient } from "@libsql/client/web"
import { createRouter } from "rouzer"
import { TursoBackendControlPlane } from "../db/persistence.js"
import type { Env } from "../env.js"
import { HttpError, assertRepo, type BackendControlPlane } from "./control-plane.js"

/** Test seams and runtime adapters injected into the backend router. */
type RouterDependencies = {
  createControlPlane?: (env: Env) => BackendControlPlane
  broadcastEvent?: (env: Env, event: RepoEvent) => Promise<void>
  handleUserStream?: (env: Env, githubUsername: string, request: Request) => Promise<Response>
}

/** Creates the backend HTTP router over the current control-plane implementation. */
export function createBackendRouter(dependencies: RouterDependencies = {}) {
  const createControlPlane = dependencies.createControlPlane ?? createTursoControlPlane
  const broadcastEvent = dependencies.broadcastEvent ?? noopBroadcast
  const handleUserStream = dependencies.handleUserStream ?? defaultHandleUserStream

  return createRouter<Env>({ debug: false }).use(routes, {
    authDeviceStartRoute: {
      POST: async (ctx) => {
        try {
          const controlPlane = createControlPlane(readEnv(ctx))
          return await controlPlane.startDeviceFlow(ctx.body)
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    authDeviceCompleteRoute: {
      POST: async (ctx) => {
        try {
          const controlPlane = createControlPlane(readEnv(ctx))
          return await controlPlane.completeDeviceFlow(ctx.body)
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    authSessionRoute: {
      GET: async (ctx) => {
        try {
          const controlPlane = createControlPlane(readEnv(ctx))
          const token = readBearerToken(ctx.headers.authorization)
          return await controlPlane.getSession(token)
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    prCreateRoute: {
      POST: async (ctx) => {
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
    },
    prManagedRoute: {
      GET: async (ctx) => {
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
    },
    prReplyRoute: {
      POST: async (ctx) => {
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
    githubWebhookRoute: {
      POST: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const event = await controlPlane.handleGitHubWebhook(ctx.body)
          await broadcastEvent(env, event)
          return event
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    repoStreamRoute: {
      GET: async (ctx) => {
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

/** Rehydrates the worker environment values used by the backend control plane. */
function readEnv(ctx: { env: <K extends keyof Env>(key: K) => Env[K] }): Env {
  return {
    TURSO_DB_URL: ctx.env("TURSO_DB_URL"),
    TURSO_DB_AUTH_TOKEN: ctx.env("TURSO_DB_AUTH_TOKEN"),
    GITHUB_APP_ID: ctx.env("GITHUB_APP_ID"),
    GITHUB_APP_PRIVATE_KEY: ctx.env("GITHUB_APP_PRIVATE_KEY"),
    USER_STREAM: ctx.env("USER_STREAM"),
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
  const message = error instanceof Error ? error.message : "Unknown error"
  return Response.json({ error: message }, { status: statusCode })
}

const router = createBackendRouter()

export default router

import { createClient } from "@libsql/client/web"
import type { RepoEvent } from "@goddard-ai/schema/backend"
import * as routes from "@goddard-ai/schema/backend/routes"
import { createRouter } from "rouzer"
import { TursoBackendControlPlane } from "../db/persistence.ts"
import { HttpError, assertRepo, type BackendControlPlane } from "./control-plane.ts"
import type { Env } from "../env.ts"
import { handleGitHubWebhookRequest } from "../github-webhooks.ts"

type RouterDependencies = {
  createControlPlane?: (env: Env) => BackendControlPlane
  broadcastToRepo?: (env: Env, owner: string, repo: string, event: RepoEvent) => Promise<void>
  handleRepoStream?: (env: Env, owner: string, repo: string, request: Request) => Promise<Response>
}

export function createBackendRouter(dependencies: RouterDependencies = {}) {
  const createControlPlane = dependencies.createControlPlane ?? createTursoControlPlane
  const broadcastToRepo = dependencies.broadcastToRepo ?? noopBroadcast
  const handleRepoStream = dependencies.handleRepoStream ?? defaultHandleRepoStream

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
          const pr = await controlPlane.createPr(token, ctx.body)

          await broadcastToRepo(env, pr.owner, pr.repo, {
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
          return await handleGitHubWebhookRequest({
            env,
            request: ctx.request,
            handleWebhookEvent: (input) => controlPlane.handleGitHubWebhook(input),
            broadcastEvent: (event) => broadcastToRepo(env, event.owner, event.repo, event),
          })
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
    githubWebhookEventRoute: {
      POST: async (ctx) => {
        try {
          const env = readEnv(ctx)
          const controlPlane = createControlPlane(env)
          const event = await controlPlane.handleGitHubWebhook(ctx.body)
          await broadcastToRepo(env, event.owner, event.repo, event)
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
          const { owner, repo } = ctx.query
          assertRepo(owner, repo)
          await controlPlane.getSession(token)

          return await handleRepoStream(env, owner, repo, ctx.request)
        } catch (error) {
          return toErrorResponse(error)
        }
      },
    },
  })
}

function createTursoControlPlane(env: Env): BackendControlPlane {
  const client = createClient({
    url: env.TURSO_DB_URL,
    authToken: env.TURSO_DB_AUTH_TOKEN,
  })

  return new TursoBackendControlPlane(client as any)
}

async function noopBroadcast(
  _env: Env,
  _owner: string,
  _repo: string,
  _event: RepoEvent,
): Promise<void> {
  // No-op: the caller (e.g. worker.ts) should provide a real implementation.
}

async function defaultHandleRepoStream(
  _env: Env,
  _owner: string,
  _repo: string,
  _request: Request,
): Promise<Response> {
  return new Response("SSE handler not configured", { status: 501 })
}

function readEnv(ctx: { env: <K extends keyof Env>(key: K) => Env[K] }): Env {
  return {
    TURSO_DB_URL: ctx.env("TURSO_DB_URL"),
    TURSO_DB_AUTH_TOKEN: ctx.env("TURSO_DB_AUTH_TOKEN"),
    GITHUB_APP_ID: ctx.env("GITHUB_APP_ID"),
    GITHUB_APP_PRIVATE_KEY: ctx.env("GITHUB_APP_PRIVATE_KEY"),
    GITHUB_WEBHOOK_SECRET: ctx.env("GITHUB_WEBHOOK_SECRET"),
  }
}

function readBearerToken(header: string): string {
  if (!header || !header.startsWith("Bearer ")) {
    throw new HttpError(401, "Missing Bearer token")
  }

  return header.slice("Bearer ".length)
}

function toErrorResponse(error: unknown): Response {
  const statusCode = error instanceof HttpError ? error.statusCode : 500
  const message = error instanceof Error ? error.message : "Unknown error"
  return Response.json({ error: message }, { status: statusCode })
}

const router = createBackendRouter()

export default router

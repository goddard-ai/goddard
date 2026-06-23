import { authBackendRoutes } from "@goddard-ai/auth/backend"
import {
  composeBackendEvents,
  composeBackendEventSources,
  composeBackendRoutes,
  type BackendEventEnvelope,
} from "@goddard-ai/backend-plugin"
import {
  githubBackendEventSources,
  githubBackendRoutes,
  GitHubWebhookError,
  normalizeGitHubWebhookRequest,
  readGitHubWebhookRequest,
} from "@goddard-ai/github/backend"
import {
  pullRequestBackendEventSources,
  pullRequestBackendRoutes,
  pullRequestRemoteRepoEventHandler,
} from "@goddard-ai/pull-request/backend"
import {
  dispatchRemoteRepoEvent,
  remoteRepoBackendEvents,
  remoteRepoBackendRoutes,
  type RemoteRepoEventHandler,
} from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import { createClient } from "@libsql/client/web"
import { getErrorMessage } from "radashi"
import { createRouter } from "rouzer"

import { TursoBackendControlPlane } from "../db/persistence.ts"
import type { Env } from "../env.ts"
import { assertRepo, HttpError, type BackendControlPlane } from "./control-plane.ts"
import type { BackendPrincipal } from "./events.ts"

type RemoteRepoBackendEvent = BackendEventEnvelope<"remote_repo.event.received", RepoEvent>

const backendRoutes = composeBackendRoutes([
  authBackendRoutes,
  githubBackendRoutes,
  pullRequestBackendRoutes,
  remoteRepoBackendRoutes,
])
const backendEvents = composeBackendEvents([remoteRepoBackendEvents])
const backendEventSources = composeBackendEventSources(
  [githubBackendEventSources, pullRequestBackendEventSources],
  backendEvents,
)

export type BackendEventPublication = {
  readonly source: keyof typeof backendEventSources & string
  readonly event: RemoteRepoBackendEvent
}

/** Test seams and runtime adapters injected into the backend router. */
type RouterDependencies = {
  createControlPlane?: (env: Env) => BackendControlPlane
  broadcastEvent?: (env: Env, publication: BackendEventPublication) => Promise<void>
  handleUserStream?: (env: Env, principal: BackendPrincipal, request: Request) => Promise<Response>
  remoteRepoEventHandlers?: readonly RemoteRepoEventHandler[]
}

/** Creates the backend HTTP router over the current control-plane implementation. */
export function createBackendRouter(dependencies: RouterDependencies = {}) {
  const createControlPlane = dependencies.createControlPlane ?? createTursoControlPlane
  const broadcastEvent = dependencies.broadcastEvent ?? noopBroadcast
  const handleUserStream = dependencies.handleUserStream ?? defaultHandleUserStream
  const remoteRepoEventHandlers = dependencies.remoteRepoEventHandlers ?? [
    pullRequestRemoteRepoEventHandler,
  ]
  const publishEvent = createBackendEventPublisher(remoteRepoEventHandlers, broadcastEvent)

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

          await publishEvent(env, {
            source: "pull-request",
            event: {
              name: "remote_repo.event.received",
              payload: {
                type: "pr.created",
                owner: pr.owner,
                repo: pr.repo,
                prNumber: pr.number,
                title: pr.title,
                author: pr.createdBy,
                createdAt: pr.createdAt,
              },
            },
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
          const input = await readGitHubWebhookRequest(ctx.request, env.GITHUB_WEBHOOK_SECRET)
          const event = normalizeGitHubWebhookRequest(input)
          if (!event) {
            return { ignored: true }
          }

          await publishEvent(env, {
            source: "github",
            event,
          })
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
          const principal = await controlPlane.getPrincipal(token)

          return await handleUserStream(env, principal, ctx.request)
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

function createBackendEventPublisher(
  remoteRepoEventHandlers: readonly RemoteRepoEventHandler[],
  broadcastEvent: (env: Env, publication: BackendEventPublication) => Promise<void>,
) {
  return async (env: Env, publication: BackendEventPublication) => {
    assertBackendEventPublication(publication)
    await dispatchRemoteRepoEvent(publication.event.payload, remoteRepoEventHandlers)
    await broadcastEvent(env, publication)
  }
}

export async function authorizeBackendEventPublication(
  principal: BackendPrincipal,
  publication: BackendEventPublication,
) {
  assertBackendEventPublication(publication)
  const source = backendEventSources[publication.source] as {
    authorize: (input: {
      readonly principal: BackendPrincipal
      readonly event: RemoteRepoBackendEvent
    }) => boolean | Promise<boolean>
  }
  return source.authorize({
    principal,
    event: publication.event,
  })
}

function assertBackendEventPublication(publication: BackendEventPublication) {
  const source = backendEventSources[publication.source]
  if (!source) {
    throw new HttpError(500, `Unknown backend event source: ${publication.source}`)
  }

  if (!source.produces.includes(publication.event.name)) {
    throw new HttpError(
      500,
      `Backend event source ${publication.source} cannot produce event: ${publication.event.name}`,
    )
  }

  const definition = backendEvents[publication.event.name]
  if (!definition) {
    throw new HttpError(500, `Unknown backend event: ${publication.event.name}`)
  }

  const payload = definition.payload.safeParse(publication.event.payload)
  if (!payload.success) {
    throw new HttpError(500, `Invalid backend event payload: ${publication.event.name}`)
  }
}

/** Provides a safe default when the worker host does not supply event broadcasting. */
async function noopBroadcast(_env: Env, _publication: BackendEventPublication): Promise<void> {
  // No-op: the caller (e.g. worker.js) should provide a real implementation.
}

/** Returns a clear placeholder response when server-sent events are not wired in. */
async function defaultHandleUserStream(
  _env: Env,
  _principal: BackendPrincipal,
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
    GITHUB_WEBHOOK_SECRET: ctx.env("GITHUB_WEBHOOK_SECRET"),
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
  const statusCode =
    error instanceof HttpError || error instanceof GitHubWebhookError ? error.statusCode : 500
  const message = getErrorMessage(error)
  return Response.json({ error: message }, { status: statusCode })
}

const router = createBackendRouter()

export default router

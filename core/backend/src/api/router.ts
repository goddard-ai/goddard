import { createRouter, ndjson } from "@goddard-ai/backend-plugin"
import {
  getDefaultBackendPluginComposition,
  handleDefaultGitHubWebhookRequest,
} from "@goddard-ai/default-features/backend"
import {
  createRemoteRepoBackendEvent,
  type RemoteRepoBackendEvent,
} from "@goddard-ai/remote-repo/backend"
import type { BackendEventStreamRequest, RepoEvent } from "@goddard-ai/remote-repo/schema"
import { createClient } from "@libsql/client/web"
import { getErrorMessage } from "radashi"

import { TursoBackendControlPlane } from "../db/persistence.ts"
import type { Env } from "../env.ts"
import { createReadyNdjsonResponse } from "../utils.ts"
import { assertRepo, HttpError, type BackendControlPlane } from "./control-plane.ts"
import { getPrincipalStreamKey, type BackendPrincipal } from "./events.ts"

const backendPlugins = getDefaultBackendPluginComposition()
const backendRoutes = backendPlugins.routes
const backendEvents = backendPlugins.events
const backendEventSources = backendPlugins.eventSources
const backendProviders = backendPlugins.providers
const backendNdjsonRouterPlugin = {
  ...ndjson.routerPlugin,
  encode(value: unknown) {
    return createReadyNdjsonResponse(value as ndjson.NdjsonSource)
  },
}

export type BackendEventPublication = {
  readonly source: keyof typeof backendEventSources & string
  readonly event: RemoteRepoBackendEvent
}

/** Test seams and runtime adapters injected into the backend router. */
type RouterDependencies = {
  createControlPlane?: (env: Env) => BackendControlPlane
  broadcastEvent?: (env: Env, publication: BackendEventPublication) => Promise<void>
  handleUserEvents?: (
    env: Env,
    streamKey: string,
    filter: BackendEventStreamRequest,
    request: Request,
  ) => Promise<AsyncIterable<RepoEvent>> | AsyncIterable<RepoEvent>
}

/** Creates the backend HTTP router over the current control-plane implementation. */
export function createBackendRouter(dependencies: RouterDependencies = {}) {
  const createControlPlane = dependencies.createControlPlane ?? createTursoControlPlane
  const broadcastEvent = dependencies.broadcastEvent ?? noopBroadcast
  const handleUserEvents = dependencies.handleUserEvents ?? defaultHandleUserEvents
  const publishEvent = createBackendEventPublisher(broadcastEvent)

  return createRouter<Env>({ debug: false, plugins: [backendNdjsonRouterPlugin] }).use(
    backendRoutes,
    {
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
              source: "remote-repo",
              event: createRemoteRepoBackendEvent({
                type: "pr.created",
                provider: pr.provider,
                owner: pr.owner,
                repo: pr.repo,
                prNumber: pr.number,
                title: pr.title,
                author: pr.createdBy,
                createdAt: pr.createdAt,
              }),
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
            const { provider, owner, repo, prNumber } = ctx.query
            assertRepo(owner, repo)
            const managed = await controlPlane.isManagedPr(
              provider,
              owner,
              repo,
              prNumber,
              session.principal.id,
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
            const event = await handleDefaultGitHubWebhookRequest(
              ctx.request,
              env.GITHUB_WEBHOOK_SECRET,
            )
            if (!event) {
              return new Response(null, { status: 204 })
            }

            await publishEvent(env, {
              source: "remote-repo",
              event,
            })
            return event
          } catch (error) {
            return toErrorResponse(error)
          }
        },
      },
      events: {
        stream: async (ctx) => {
          try {
            const env = readEnv(ctx)
            const controlPlane = createControlPlane(env)
            const token = readBearerToken(ctx.headers.authorization)
            const principal = await controlPlane.getPrincipal(token)

            return await handleUserEvents(
              env,
              getPrincipalStreamKey(principal),
              ctx.body,
              ctx.request,
            )
          } catch (error) {
            return toErrorResponse(error)
          }
        },
      },
    },
  )
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
  broadcastEvent: (env: Env, publication: BackendEventPublication) => Promise<void>,
) {
  return async (env: Env, publication: BackendEventPublication) => {
    assertBackendEventPublication(publication)
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
      readonly providers: typeof backendProviders
    }) => boolean | Promise<boolean>
  }
  return source.authorize({
    principal,
    event: publication.event,
    providers: backendProviders,
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

/** Returns a clear placeholder error when backend event streaming is not wired in. */
async function defaultHandleUserEvents(
  _env: Env,
  _githubUsername: string,
  _filter: BackendEventStreamRequest,
  _request: Request,
): Promise<AsyncIterable<RepoEvent>> {
  throw new HttpError(501, "Event stream handler not configured")
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
    error instanceof HttpError ? error.statusCode : hasStatusCode(error) ? error.statusCode : 500
  const message = getErrorMessage(error)
  return Response.json({ error: message }, { status: statusCode })
}

function hasStatusCode(error: unknown): error is { statusCode: number } {
  return (
    typeof error === "object" &&
    error !== null &&
    "statusCode" in error &&
    typeof (error as { statusCode: unknown }).statusCode === "number"
  )
}

const router = createBackendRouter()

export default router

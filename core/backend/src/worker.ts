import { matchesEventEnvelopeFilter, type EventEnvelopeFilter } from "@goddard-ai/event-filter"
import type { RemoteRepoStreamService } from "@goddard-ai/remote-repo/backend"
import adapter from "@hattip/adapter-cloudflare-workers/no-static"
import { createClient } from "@libsql/client/web"

import { getPrincipalStreamKey } from "./api/events.ts"
import {
  authorizeBackendEventPublication,
  createBackendRouter,
  type BackendEventPublication,
} from "./api/router.ts"
import { TursoBackendControlPlane } from "./db/persistence.ts"
import type { Env } from "./env.ts"
import { createSseSession } from "./utils.ts"

const router = createBackendRouter({
  broadcastEvent: async (env, publication) => {
    const principalId = await createWorkerStreamService(env).resolveEventOwner(
      publication.event.payload,
    )
    if (!principalId) {
      return
    }

    const controlPlane = createWorkerControlPlane(env)
    const principal = await controlPlane.getPrincipalForId(principalId)
    if (!(await authorizeBackendEventPublication(principal, publication))) {
      return
    }

    await getUserStreamStub(env, getPrincipalStreamKey(principal)).fetch(
      "https://user-stream.internal/publish",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ event: publication.event }),
      },
    )
  },
  handleUserStream: async (env, principal, _request) => {
    const requestUrl = new URL(_request.url)
    return getUserStreamStub(env, getPrincipalStreamKey(principal)).fetch(
      `https://user-stream.internal/subscribe${requestUrl.search}`,
    )
  },
})

/** Cloudflare Worker entrypoint for the backend API and user-scoped stream runtime. */
const worker = {
  fetch: adapter(router),
} satisfies ExportedHandler<Env>

export default worker

/** User-scoped Durable Object that owns SSE subscribers for one Goddard user. */
export class UserStream {
  readonly #subscriptions = new Set<{
    readonly sink: ReturnType<typeof createSseSession>["sink"]
    readonly filter: EventEnvelopeFilter
  }>()

  fetch(request: Request): Promise<Response> | Response {
    const url = new URL(request.url)
    if (request.method === "POST" && url.pathname === "/publish") {
      return this.#publish(request)
    }
    if (request.method === "GET" && url.pathname === "/subscribe") {
      return this.#subscribe(request)
    }

    return new Response("Not found", { status: 404 })
  }

  async #publish(request: Request): Promise<Response> {
    const payload = (await request.json()) as { event: BackendEventPublication["event"] }
    const frame = JSON.stringify(payload.event)

    for (const subscription of this.#subscriptions) {
      if (!matchesEventEnvelopeFilter(payload.event, subscription.filter)) {
        continue
      }
      try {
        subscription.sink.send(frame)
      } catch {
        this.#subscriptions.delete(subscription)
        subscription.sink.close?.()
      }
    }

    return new Response(null, { status: 204 })
  }

  #subscribe(request: Request): Response {
    const session = createSseSession(() => {
      this.#deleteSink(session.sink)
    })
    const subscription = {
      sink: session.sink,
      filter: readStreamFilter(request),
    }

    this.#subscriptions.add(subscription)
    request.signal.addEventListener(
      "abort",
      () => {
        this.#subscriptions.delete(subscription)
        session.sink.close?.()
      },
      { once: true },
    )

    return session.response
  }

  #deleteSink(sink: ReturnType<typeof createSseSession>["sink"]) {
    for (const subscription of this.#subscriptions) {
      if (subscription.sink === sink) {
        this.#subscriptions.delete(subscription)
      }
    }
  }
}

function readStreamFilter(request: Request): EventEnvelopeFilter {
  const value = new URL(request.url).searchParams.get("filter")
  if (!value) {
    return {}
  }

  try {
    return JSON.parse(value) as EventEnvelopeFilter
  } catch {
    return {}
  }
}

function createWorkerStreamService(env: Env): Pick<RemoteRepoStreamService, "resolveEventOwner"> {
  return createWorkerControlPlane(env)
}

function createWorkerControlPlane(env: Env): TursoBackendControlPlane {
  return new TursoBackendControlPlane(
    createClient({
      url: env.TURSO_DB_URL,
      authToken: env.TURSO_DB_AUTH_TOKEN,
    }) as any,
  )
}

function getUserStreamStub(env: Env, principalId: string) {
  if (!env.USER_STREAM) {
    throw new Error("USER_STREAM Durable Object binding is not configured")
  }

  return env.USER_STREAM.get(env.USER_STREAM.idFromName(principalId))
}

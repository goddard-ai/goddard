import { ndjson } from "@goddard-ai/backend-plugin"
import type { RemoteRepoStreamService } from "@goddard-ai/remote-repo/backend"
import type { BackendEventStreamRequest, RepoEvent } from "@goddard-ai/remote-repo/schema"
import adapter from "@hattip/adapter-cloudflare-workers/no-static"
import { createClient } from "@libsql/client/web"

import { getPrincipalStreamKey } from "./api/events.ts"
import {
  authorizeBackendEventPublication,
  createBackendRouter,
} from "./api/router.ts"
import { TursoBackendControlPlane } from "./db/persistence.ts"
import type { Env } from "./env.ts"
import {
  createEventQueue,
  createReadyNdjsonResponse,
  filterRepoEvent,
  type EventQueue,
} from "./utils.ts"

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
        body: JSON.stringify({ event: publication.event.payload }),
      },
    )
  },
  handleUserEvents: async (env, githubUsername, filter, request) => {
    const response = await getUserStreamStub(env, githubUsername).fetch(
      "https://user-stream.internal/subscribe",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(filter ?? {}),
        signal: request.signal,
      },
    )

    if (!response.ok) {
      throw new Error(`User stream subscription failed: ${response.status}`)
    }
    if (!response.body) {
      throw new Error("User stream subscription did not include a body")
    }

    return ndjson.decodeNdjson<RepoEvent>(response.body)
  },
})

/** Cloudflare Worker entrypoint for the backend API and user-scoped stream runtime. */
const worker = {
  fetch: adapter(router),
} satisfies ExportedHandler<Env>

export default worker

/** User-scoped Durable Object that owns NDJSON subscribers for one Goddard user. */
export class UserStream {
  readonly #queues = new Set<EventQueue<RepoEvent>>()

  fetch(request: Request): Promise<Response> | Response {
    const url = new URL(request.url)
    if (request.method === "POST" && url.pathname === "/publish") {
      return this.#publish(request)
    }
    if (request.method === "POST" && url.pathname === "/subscribe") {
      return this.#subscribe(request)
    }

    return new Response("Not found", { status: 404 })
  }

  async #publish(request: Request): Promise<Response> {
    const payload = (await request.json()) as { event: RepoEvent }

    for (const queue of this.#queues) {
      queue.publish(payload.event)
    }

    return new Response(null, { status: 204 })
  }

  async #subscribe(request: Request): Promise<Response> {
    const filter = (await request.json().catch(() => ({}))) as BackendEventStreamRequest
    const queue = createEventQueue<RepoEvent>(
      (event) => filterRepoEvent(event, filter),
      () => {
        this.#queues.delete(queue)
      },
    )

    this.#queues.add(queue)

    return createReadyNdjsonResponse(queue)
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

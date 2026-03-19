import adapter from "@hattip/adapter-cloudflare-workers/no-static"
import { createClient } from "@libsql/client/web"
import type { RepoEvent } from "@goddard-ai/schema/backend"
import type { Env } from "./env.ts"
import { createBackendRouter } from "./api/router.ts"
import { TursoBackendControlPlane } from "./db/persistence.ts"
import { createSseSession } from "./utils.ts"

const router = createBackendRouter({
  broadcastEvent: async (env, event) => {
    const githubUsername = await createWorkerControlPlane(env).resolveEventOwner?.(event)
    if (!githubUsername) {
      return
    }

    await getUserStreamStub(env, githubUsername).fetch("https://user-stream.internal/publish", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ event }),
    })
  },
  handleUserStream: async (env, githubUsername, _request) => {
    return getUserStreamStub(env, githubUsername).fetch("https://user-stream.internal/subscribe")
  },
})

/** Cloudflare Worker entrypoint for the backend API and user-scoped stream runtime. */
const worker = {
  fetch: adapter(router),
} satisfies ExportedHandler<Env>

export default worker

/** User-scoped Durable Object that owns SSE subscribers for one Goddard user. */
export class UserStream {
  readonly #sinks = new Set<ReturnType<typeof createSseSession>["sink"]>()

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
    const payload = (await request.json()) as { event: RepoEvent }
    const frame = JSON.stringify({ event: payload.event })

    for (const sink of this.#sinks) {
      try {
        sink.send(frame)
      } catch {
        this.#sinks.delete(sink)
        sink.close?.()
      }
    }

    return new Response(null, { status: 204 })
  }

  #subscribe(request: Request): Response {
    const session = createSseSession(() => {
      this.#sinks.delete(session.sink)
    })

    this.#sinks.add(session.sink)
    request.signal.addEventListener(
      "abort",
      () => {
        this.#sinks.delete(session.sink)
        session.sink.close?.()
      },
      { once: true },
    )

    return session.response
  }
}

function createWorkerControlPlane(env: Env): TursoBackendControlPlane {
  return new TursoBackendControlPlane(
    createClient({
      url: env.TURSO_DB_URL,
      authToken: env.TURSO_DB_AUTH_TOKEN,
    }) as any,
  )
}

function getUserStreamStub(env: Env, githubUsername: string) {
  if (!env.USER_STREAM) {
    throw new Error("USER_STREAM Durable Object binding is not configured")
  }

  return env.USER_STREAM.get(env.USER_STREAM.idFromName(githubUsername))
}

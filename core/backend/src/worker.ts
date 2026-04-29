import type { RemoteRepoStreamService } from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import adapter from "@hattip/adapter-cloudflare-workers/no-static"
import { createClient } from "@libsql/client/web"

import { HttpError } from "./api/control-plane.ts"
import { createBackendRouter, type CloudSessionHandlerOptions } from "./api/router.ts"
import { CloudSession } from "./cloud-session.ts"
import { TursoBackendControlPlane } from "./db/persistence.ts"
import type { Env } from "./env.ts"
import { createSseSession } from "./utils.ts"

const router = createBackendRouter({
  broadcastEvent: async (env, event) => {
    const githubUsername = await createWorkerStreamService(env).resolveEventOwner(event)
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
  handleCloudSession: async (env, githubUsername, request, options) => {
    return forwardCloudSessionRequest(env, githubUsername, request, options)
  },
})

const handleRouterFetch = adapter(router)

/** Cloudflare Worker entrypoint for the backend API and user-scoped stream runtime. */
const worker = {
  async fetch(request, env, context) {
    const testResponse = await handleTestRequest(request, env)
    if (testResponse) {
      return testResponse
    }

    return handleRouterFetch(request, env, context)
  },
} satisfies ExportedHandler<Env>

export default worker
export { CloudSession }

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

function createWorkerStreamService(env: Env): Pick<RemoteRepoStreamService, "resolveEventOwner"> {
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

async function forwardCloudSessionRequest(
  env: Env,
  githubUsername: string,
  request: Request,
  options: CloudSessionHandlerOptions,
) {
  const sourceUrl = new URL(request.url)
  const targetUrl = new URL(`https://cloud-session.internal${options.pathname}`)
  targetUrl.search = sourceUrl.search
  const headers = new Headers(request.headers)
  headers.set("x-goddard-cloud-session-owner", githubUsername)

  let body: BodyInit | undefined
  if (options.body !== undefined) {
    headers.set("content-type", "application/json")
    body = JSON.stringify(options.body)
  } else if (request.method !== "GET" && request.method !== "HEAD") {
    body = await request.arrayBuffer()
  }

  return await getCloudSessionStub(env, githubUsername, options.sessionId).fetch(
    new Request(targetUrl.toString(), {
      method: request.method,
      headers,
      body,
    }),
  )
}

function getCloudSessionStub(env: Env, githubUsername: string, sessionId: string) {
  if (!env.CLOUD_SESSION) {
    throw new HttpError(500, "CLOUD_SESSION Durable Object binding is not configured")
  }

  return env.CLOUD_SESSION.get(env.CLOUD_SESSION.idFromName(`${githubUsername}:${sessionId}`))
}

async function handleTestRequest(request: Request, env: Env) {
  const url = new URL(request.url)
  if (!url.pathname.startsWith("/__test/")) {
    return null
  }

  if (env.GODDARD_BACKEND_TEST_MODE !== "1") {
    return new Response("Not found", { status: 404 })
  }

  if (url.pathname === "/__test/health") {
    return new Response(null, { status: 204 })
  }

  const match = /^\/__test\/cloud\/sessions\/([^/]+)\/([^/]+)$/.exec(url.pathname)
  if (!match) {
    return new Response("Not found", { status: 404 })
  }

  const sessionId = decodeURIComponent(match[1])
  const action = match[2]
  if (action === "create" && request.method === "POST") {
    const input = await readOptionalJson(request)
    return await forwardCloudSessionRequest(env, "__test__", request, {
      sessionId,
      pathname: "/create",
      body: { ...input, sessionId },
    })
  }
  if (action === "sync" && request.method === "GET") {
    return await forwardCloudSessionRequest(env, "__test__", request, {
      sessionId,
      pathname: "/sync",
    })
  }
  if (action === "commands" && request.method === "POST") {
    return await forwardCloudSessionRequest(env, "__test__", request, {
      sessionId,
      pathname: "/commands",
      body: await readOptionalJson(request),
    })
  }
  if (action === "harness" && request.method === "GET") {
    return await forwardCloudSessionRequest(env, "__test__", request, {
      sessionId,
      pathname: "/harness",
    })
  }

  return new Response("Not found", { status: 404 })
}

async function readOptionalJson(request: Request) {
  const text = await request.text()
  return text.trim() ? JSON.parse(text) : {}
}

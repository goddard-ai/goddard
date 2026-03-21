import type { RepoEventRecord, StreamMessage } from "@goddard-ai/schema/backend"
import adapter from "@hattip/adapter-cloudflare-workers/no-static"
import { createBackendRouter } from "./api/router.js"
import type { Env } from "./env.js"

/** Message body delivered to one user-scoped stream durable object for fan-out. */
type PublishRequestBody = {
  githubUsername: string
  record: RepoEventRecord
}

/** Socket attachment persisted across Durable Object hibernation cycles. */
type UserStreamAttachment = {
  githubUsername: string
}

/** Minimal Durable Object hibernation surface used by the user stream runtime. */
type UserStreamContext = Pick<
  DurableObjectState,
  "acceptWebSocket" | "getWebSockets" | "setWebSocketAutoResponse"
>

/** Runtime hooks used to create Cloudflare-specific WebSocket primitives. */
type UserStreamRuntime = {
  createWebSocketPair: () => { client: WebSocket; server: WebSocket }
  createAutoResponsePair: (request: string, response: string) => WebSocketRequestResponsePair
  createUpgradeResponse: (client: WebSocket) => Response
}

const cloudflareUserStreamRuntime: UserStreamRuntime = {
  createWebSocketPair() {
    const pair = new WebSocketPair()
    return {
      client: pair[0],
      server: pair[1],
    }
  },
  createAutoResponsePair(request, response) {
    return new WebSocketRequestResponsePair(request, response)
  },
  createUpgradeResponse(client) {
    return new Response(null, { status: 101, webSocket: client })
  },
}

const router = createBackendRouter({
  broadcastEvent: async (env, persistedEvent) => {
    await getUserStreamStub(env, persistedEvent.githubUsername).fetch(
      "https://user-stream.internal/publish",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          githubUsername: persistedEvent.githubUsername,
          record: persistedEvent.record,
        }),
      },
    )
  },
  handleUserStream: async (env, githubUsername, request) => {
    const url = new URL(request.url)
    url.protocol = "https:"
    url.hostname = "user-stream.internal"
    url.pathname = "/stream"

    const headers = new Headers(request.headers)
    headers.set("x-github-username", githubUsername)

    const upgradeRequest = new Request(url, {
      method: request.method,
      headers,
      body: request.body,
    })
    return getUserStreamStub(env, githubUsername).fetch(upgradeRequest)
  },
})

/** Cloudflare Worker entrypoint for the backend API and user-scoped stream runtime. */
const worker = {
  fetch: adapter(router),
} satisfies ExportedHandler<Env>

export default worker

/** User-scoped Durable Object that owns hibernating WebSocket subscribers for one Goddard user. */
export class UserStream {
  readonly #runtime: UserStreamRuntime

  constructor(
    readonly ctx: UserStreamContext,
    _env: Env,
    runtime: UserStreamRuntime = cloudflareUserStreamRuntime,
  ) {
    this.#runtime = runtime
    this.ctx.setWebSocketAutoResponse(this.#runtime.createAutoResponsePair("ping", "pong"))
  }

  fetch(request: Request): Promise<Response> | Response {
    const url = new URL(request.url)
    if (request.method === "POST" && url.pathname === "/publish") {
      return this.#publish(request)
    }
    if (request.method === "GET" && url.pathname === "/stream") {
      return this.#upgrade(request)
    }

    return new Response("Not found", { status: 404 })
  }

  async #publish(request: Request): Promise<Response> {
    const payload = (await request.json()) as PublishRequestBody
    const message: StreamMessage = {
      id: payload.record.id,
      event: payload.record.event,
    }
    const frame = JSON.stringify(message)

    for (const socket of this.ctx.getWebSockets()) {
      const attachment = readAttachment(socket)
      if (attachment?.githubUsername !== payload.githubUsername) {
        continue
      }

      try {
        socket.send(frame)
      } catch {
        closeSocket(socket, 1011, "publish_failed")
      }
    }

    return new Response(null, { status: 204 })
  }

  /** Accepts one authenticated stream socket and persists its routing metadata. */
  #upgrade(request: Request): Response {
    if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") {
      return new Response("Expected WebSocket upgrade", { status: 426 })
    }

    const githubUsername = request.headers.get("x-github-username")?.trim()
    if (!githubUsername) {
      return new Response("Missing stream owner", { status: 400 })
    }

    const { client, server } = this.#runtime.createWebSocketPair()
    server.serializeAttachment({ githubUsername } satisfies UserStreamAttachment)
    this.ctx.acceptWebSocket(server)
    return this.#runtime.createUpgradeResponse(client)
  }

  webSocketMessage(_socket: WebSocket, _message: string | ArrayBuffer): void {
    // Client heartbeat pings are answered at the edge by the configured auto-response pair.
  }

  webSocketClose(socket: WebSocket, code: number, reason: string, _wasClean: boolean): void {
    closeSocket(socket, code, reason)
  }

  webSocketError(socket: WebSocket, error: unknown): void {
    closeSocket(socket, 1011, error instanceof Error ? error.message : "socket_error")
  }
}

/** Reads the persisted user attachment for one hibernating WebSocket connection. */
function readAttachment(socket: WebSocket): UserStreamAttachment | null {
  try {
    return socket.deserializeAttachment() as UserStreamAttachment
  } catch {
    return null
  }
}

/** Closes one socket and ignores duplicate-close failures from already-closed peers. */
function closeSocket(socket: WebSocket, code: number, reason: string): void {
  try {
    socket.close(code, reason)
  } catch {
    // No-op: Cloudflare may already have closed the socket before the handler runs.
  }
}

/** Resolves the user-scoped Durable Object stub for one authenticated GitHub username. */
function getUserStreamStub(env: Env, githubUsername: string) {
  if (!env.USER_STREAM) {
    throw new Error("USER_STREAM Durable Object binding is not configured")
  }

  return env.USER_STREAM.get(env.USER_STREAM.idFromName(githubUsername))
}

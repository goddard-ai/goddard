import { createServer as createNodeServer } from "@hattip/adapter-node"
import type { StreamMessage } from "@goddard-ai/schema/backend"
import { WebSocketServer, type WebSocket } from "ws"
import { type BackendControlPlane, type PersistedRepoEvent } from "./api/control-plane.js"
import { InMemoryBackendControlPlane, type StreamSink } from "./api/in-memory-control-plane.js"
import { createBackendRouter } from "./api/router.js"

export * from "./api/control-plane.js"
export { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.js"
export { TursoBackendControlPlane } from "./db/persistence.js"
export * from "./github-app.js"

// Optional host and port overrides for the local Node backend server.
type StartServerOptions = {
  port?: number
  host?: string
}

/** Handle returned by the local Node backend server. */
export type BackendServer = {
  port: number
  close: () => Promise<void>
}

/** User identity captured during a local WebSocket upgrade. */
type LocalStreamConnection = {
  githubUsername: string
}

/** Starts the local Node backend server with an in-memory or injected control plane. */
export async function startBackendServer(
  controlPlane: BackendControlPlane = new InMemoryBackendControlPlane(),
  options: StartServerOptions = {},
): Promise<BackendServer> {
  const host = options.host ?? "127.0.0.1"
  const port = options.port ?? 8787

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    broadcastEvent: async (_env, event) => {
      broadcastToInMemoryStreams(controlPlane, event)
    },
    handleUserStream: async () => {
      return new Response("Expected WebSocket upgrade", { status: 426 })
    },
  })

  const httpServer = createNodeServer(router)
  const webSocketServer = new WebSocketServer({ noServer: true })

  webSocketServer.on("connection", (socket, _request, connection) => {
    const githubUsername = connection?.githubUsername
    if (!githubUsername) {
      socket.close(1008, "missing_stream_owner")
      return
    }

    const sink = createLocalWebSocketSink(socket)
    controlPlane.addStreamSocket?.(githubUsername, sink)

    socket.on("message", (data, isBinary) => {
      if (!isBinary && data.toString() === "ping") {
        socket.send("pong")
      }
    })
    socket.on("close", () => {
      controlPlane.removeStreamSocket?.(githubUsername, sink)
    })
    socket.on("error", () => {
      controlPlane.removeStreamSocket?.(githubUsername, sink)
    })
  })

  httpServer.on("upgrade", async (request, socket, head) => {
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? host}`)
    if (url.pathname !== "/stream") {
      rejectUpgrade(socket, 404, "Not found")
      return
    }

    const token = url.searchParams.get("token")?.trim()
    if (!token) {
      rejectUpgrade(socket, 401, "Missing token")
      return
    }

    try {
      const session = await controlPlane.getSession(token)
      webSocketServer.handleUpgrade(request, socket, head, (client) => {
        webSocketServer.emit("connection", client, request, {
          githubUsername: session.githubUsername,
        } satisfies LocalStreamConnection)
      })
    } catch (error) {
      rejectUpgrade(
        socket,
        401,
        error instanceof Error ? error.message : "Unauthorized WebSocket connection",
      )
    }
  })

  await new Promise<void>((resolve) => httpServer.listen(port, host, () => resolve()))

  return {
    port: Number((httpServer.address() as { port: number }).port),
    close: async () => {
      for (const socket of webSocketServer.clients) {
        socket.close()
      }
      await new Promise<void>((resolve) => webSocketServer.close(() => resolve()))
      await new Promise<void>((resolve, reject) => {
        httpServer.close((error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    },
  }
}

function broadcastToInMemoryStreams(
  controlPlane: BackendControlPlane,
  event: PersistedRepoEvent,
): void {
  if ("broadcast" in controlPlane && typeof controlPlane.broadcast === "function") {
    controlPlane.broadcast(event)
  }
}

/** Wraps one Node WebSocket connection in the minimal sink shape used by the in-memory control plane. */
function createLocalWebSocketSink(socket: WebSocket): StreamSink {
  return {
    send(message: StreamMessage) {
      socket.send(JSON.stringify(message))
    },
    close() {
      socket.close()
    },
  }
}

/** Rejects a local upgrade request with a plain HTTP response and closes the socket. */
function rejectUpgrade(
  socket: Pick<import("node:net").Socket, "write" | "destroy">,
  statusCode: number,
  message: string,
): void {
  socket.write(
    `HTTP/1.1 ${statusCode} ${message}\r\nConnection: close\r\nContent-Length: ${message.length}\r\n\r\n${message}`,
  )
  socket.destroy()
}

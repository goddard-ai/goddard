import { type RepoEvent } from "@goddard-ai/schema/backend"
import { createServer as createNodeServer } from "@hattip/adapter-node"
import type { Socket } from "node:net"
import { type BackendControlPlane } from "./api/control-plane.ts"
import { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.ts"
import { createBackendRouter } from "./api/router.ts"
import { createSseSession } from "./utils.ts"

export * from "./api/control-plane.ts"
export { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.ts"
export { TursoBackendControlPlane } from "./db/persistence.ts"
export * from "./github-app.ts"

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
    handleUserStream: async (_env, githubUsername, request) => {
      const sseSession = createSseSession(() => {
        controlPlane.removeStreamSocket?.(githubUsername, sseSession.sink)
      })

      controlPlane.addStreamSocket?.(githubUsername, sseSession.sink)
      request.signal.addEventListener(
        "abort",
        () => {
          controlPlane.removeStreamSocket?.(githubUsername, sseSession.sink)
          sseSession.sink.close?.()
        },
        { once: true },
      )

      return sseSession.response
    },
  })

  const httpServer = createNodeServer(router)

  // Track raw sockets so tests can force-close long-lived stream connections on runtimes
  // where server.close() does not drain them promptly.
  const sockets = new Set<Socket>()
  httpServer.on("connection", (socket) => {
    sockets.add(socket)
    socket.on("close", () => {
      sockets.delete(socket)
    })
  })

  await new Promise<void>((resolve) => httpServer.listen(port, host, resolve))

  return {
    port: Number((httpServer.address() as { port: number }).port),
    close: async () => {
      await new Promise<void>((resolve, reject) => {
        const handleClose = (error?: Error | null) => {
          if (error) {
            // Bun/Node shutdown races can report "not running" after connections were
            // already torn down; treat that as an idempotent close instead of a real
            // failure.
            if ("code" in error && error.code === "ERR_SERVER_NOT_RUNNING") {
              resolve()
              return
            }
            reject(error)
            return
          }
          resolve()
        }

        try {
          httpServer.close(handleClose)
        } catch (error) {
          handleClose(error instanceof Error ? error : new Error(String(error)))
        }

        // Use the native bulk-close when the current runtime exposes it, but keep the
        // for-loop fallback so this can still shut down cleanly on partial Node-compat
        // surfaces.
        httpServer.closeAllConnections?.()
        for (const socket of sockets) {
          socket.destroy()
        }
      })
    },
  }
}

function broadcastToInMemoryStreams(controlPlane: BackendControlPlane, event: RepoEvent): void {
  if ("broadcast" in controlPlane && typeof controlPlane.broadcast === "function") {
    controlPlane.broadcast(event)
  }
}

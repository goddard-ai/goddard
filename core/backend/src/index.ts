import { createServer as createNodeServer } from "@hattip/adapter-node"
import { type BackendControlPlane, type PersistedRepoEvent } from "./api/control-plane.js"
import { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.js"
import { createBackendRouter } from "./api/router.js"
import { createSseSession } from "./utils.js"

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

  await new Promise<void>((resolve) => httpServer.listen(port, host, () => resolve()))

  return {
    port: Number((httpServer.address() as { port: number }).port),
    close: async () => {
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

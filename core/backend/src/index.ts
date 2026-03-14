import { createServer as createNodeServer } from "@hattip/adapter-node"
import type { Env } from "./env.ts"
import { type RepoEvent } from "@goddard-ai/schema/backend"
import { type BackendControlPlane } from "./api/control-plane.ts"
import { createBackendRouter } from "./api/router.ts"
import { createSseSession } from "./utils.ts"
import { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.ts"

export { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.ts"
export { TursoBackendControlPlane } from "./db/persistence.ts"
export * from "./api/control-plane.ts"
export * from "./github-app.ts"

type StartServerOptions = {
  port?: number
  host?: string
}

export type BackendServer = {
  port: number
  close: () => Promise<void>
}

export async function startBackendServer(
  controlPlane: BackendControlPlane = new InMemoryBackendControlPlane(),
  options: StartServerOptions = {},
): Promise<BackendServer> {
  const host = options.host ?? "127.0.0.1"
  const port = options.port ?? 8787

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    broadcastToRepo: async (_env, _owner, _repo, event) => {
      broadcastToInMemoryStreams(controlPlane, event)
    },
    handleRepoStream: async (_env, owner, repo, request) => {
      const repoKey = `${owner}/${repo}`
      const sseSession = createSseSession(() => {
        controlPlane.removeStreamSocket?.(repoKey, sseSession.sink)
      })

      controlPlane.addStreamSocket?.(repoKey, sseSession.sink)
      request.signal.addEventListener(
        "abort",
        () => {
          controlPlane.removeStreamSocket?.(repoKey, sseSession.sink)
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

function broadcastToInMemoryStreams(controlPlane: BackendControlPlane, event: RepoEvent): void {
  if ("broadcast" in controlPlane && typeof controlPlane.broadcast === "function") {
    controlPlane.broadcast(event)
  }
}

import type { Socket } from "node:net"
import {
  isRemoteRepoEventBroadcaster,
  isRemoteRepoStreamService,
  type RemoteRepoStreamEvent,
} from "@goddard-ai/remote-repo/backend"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import { createServer as createNodeServer } from "@hattip/adapter-node"
import { getErrorMessage } from "radashi"

import { type BackendControlPlane } from "./api/control-plane.ts"
import { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.ts"
import { createBackendRouter } from "./api/router.ts"

export * from "./api/control-plane.ts"
export { InMemoryBackendControlPlane } from "./api/in-memory-control-plane.ts"
export { TursoBackendControlPlane } from "./db/persistence.ts"

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
    broadcastEvent: async (_env, publication) => {
      broadcastToInMemoryStreams(controlPlane, publication.event)
    },
    handleUserEvents: (_env, githubUsername, filter) => {
      if (isRemoteRepoStreamService(controlPlane)) {
        return controlPlane.subscribeRemoteRepoEvents(githubUsername, filter)
      }

      return emptyRemoteRepoEvents()
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
          handleClose(error instanceof Error ? error : new Error(getErrorMessage(error)))
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

function broadcastToInMemoryStreams(
  controlPlane: BackendControlPlane,
  event: RemoteRepoStreamEvent,
): void {
  if (isRemoteRepoEventBroadcaster(controlPlane)) {
    controlPlane.broadcastRemoteRepoEvent(event)
  }
}

async function* emptyRemoteRepoEvents(): AsyncIterable<RepoEvent> {}

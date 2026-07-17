import { randomUUID } from "node:crypto"
import { once } from "node:events"
import type { Server } from "node:http"
import { composeIpcRoutes } from "@goddard-ai/ipc"
import { createServer } from "@goddard-ai/ipc/node"
import {
  coreDaemonIpcRoutes,
  type BrowserAccessClientRevokeRequest,
  type BrowserAccessPairingCompleteRequest,
  type BrowserAccessPairingConfirmRequest,
  type BrowserAccessPairingStartRequest,
  type BrowserAccessWebviewTokenCreateRequest,
  type DaemonEventsStreamRequest,
  type UpdateUserConfigRequest,
} from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { type DaemonSession } from "@goddard-ai/session/schema"
import { getErrorMessage } from "radashi"

import {
  createBrowserAccessService,
  resolveBrowserAccessRuntimeConfig,
  runBrowserAccessRequestContext,
} from "../browser-access.ts"
import { IpcRequestContext } from "../context.ts"
import { createDebug, createLogger, readSessionIdForLog } from "../logging.ts"
import type { DaemonRuntime } from "../runtime.ts"
import { createUserConfigService } from "../user-config.ts"
import type { DaemonServer } from "./types.ts"

export async function startDaemonServer(
  runtime: DaemonRuntime,
  options: {
    port?: number
  } = {},
): Promise<DaemonServer> {
  const logger = createLogger()
  const debug = createDebug("ipc.server")
  const { configManager, store } = runtime
  const rootConfig = await configManager.getRootConfig()
  const browserAccessConfig = resolveBrowserAccessRuntimeConfig(
    rootConfig.config.daemon?.browserAccess,
  )
  const browserAccessService = createBrowserAccessService(store, browserAccessConfig)
  const userConfigService = createUserConfigService()
  const ipcHandlers = {
    config: {
      get: () => userConfigService.get(),
      update: ({ body }: { body: UpdateUserConfigRequest }) => userConfigService.update(body),
    },
    daemon: {
      health: async () => ({ ok: true }),
      browserAccess: {
        pairing: {
          start: ({ body }: { body: BrowserAccessPairingStartRequest }) =>
            browserAccessService.startPairing(body),
          confirm: ({ body }: { body: BrowserAccessPairingConfirmRequest }) =>
            browserAccessService.confirmPairing(body),
          complete: ({ body }: { body: BrowserAccessPairingCompleteRequest }) =>
            browserAccessService.completePairing(body),
        },
        client: {
          list: () => browserAccessService.listClients(),
          revoke: ({ body }: { body: BrowserAccessClientRevokeRequest }) =>
            browserAccessService.revokeClient(body),
        },
        webviewToken: {
          create: ({ body }: { body: BrowserAccessWebviewTokenCreateRequest }) =>
            browserAccessService.createDesktopWebviewToken(body),
        },
      },
    },
    ...runtime.ipcHandlers,
    events: {
      stream: (ctx: { body: DaemonEventsStreamRequest; request: Request }) => {
        return runtime.events.stream(ctx.body ?? {}, ctx.request.signal)
      },
    },
  }

  const ipcServer = createServer({
    port: options.port ?? runtime.runtimeConfig.port,
    routes: composeIpcRoutes([coreDaemonIpcRoutes, runtime.ipcRoutes as any]),
    handlers: ipcHandlers as any,
    browserAccess: {
      allowedOrigins: browserAccessConfig.allowedOrigins,
      isAllowedOrigin: browserAccessConfig.isAllowedOrigin,
      authorizeRequest: browserAccessService.authorizeRequest,
    },
    runHandler: ({ payload, request }, handler) => {
      const context: IpcRequestContext = {
        opId: randomUUID(),
        sessionId: readSessionIdForLog(payload) ?? null,
        setSessionId(sessionId: DaemonSession["id"]) {
          context.sessionId = sessionId
        },
      }
      return IpcRequestContext.run(context, () => runBrowserAccessRequestContext(request, handler))
    },
    onRequestReceived: ({ name, payload }) => {
      debug("ipc.request_received", {
        requestName: name,
        method: name,
        payload,
      })
    },
    onResponseSent: ({ name, response, durationMs }) => {
      const responseSessionId = readSessionIdForLog(response)
      if (responseSessionId) {
        const context = requireIpcRequestContext()
        context.setSessionId(responseSessionId)
      }

      debug("ipc.response_sent", {
        requestName: name,
        method: name,
        durationMs,
        response,
      })
    },
    onRequestFailed: ({ name, error, durationMs }) => {
      logger.log("ipc.request_failed", {
        requestName: name,
        method: name,
        durationMs,
        errorMessage: getErrorMessage(error),
      })
    },
  })

  await once(ipcServer.server, "listening")
  const port = readBoundTcpPort(ipcServer.server)
  const daemonUrl = createDaemonUrl(port)
  runtime.setDaemonUrl(daemonUrl)

  logger.log("ipc.server_listening", {
    port,
    daemonUrl,
  })
  let closed = false

  return {
    daemonUrl,
    port,
    close: async () => {
      if (closed) {
        return
      }
      closed = true
      logger.log("ipc.server_closing", {
        port,
        daemonUrl,
      })
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error?: Error) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
      logger.log("ipc.server_closed", {
        port,
        daemonUrl,
      })
    },
  }
}

function requireIpcRequestContext() {
  const context = IpcRequestContext.get()
  if (!context) {
    throw new Error("IPC request context is unavailable")
  }

  return context
}

function readBoundTcpPort(server: Server) {
  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("IPC server did not bind to a TCP port")
  }

  return address.port
}

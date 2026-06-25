import * as http from "node:http"
import { createRouter, getResponsePluginMarkerId, type RouteRequestHandlerMap } from "rouzer"
import type { HttpAction, HttpRouteTree } from "rouzer/http"
import * as ndjson from "rouzer/ndjson"

import { IpcClientError } from "../errors.ts"

const INTERNAL_SERVER_ERROR_MESSAGE = "Internal server error"

/** Returns the safe client-facing status code and message for one IPC server failure. */
function getErrorResponse(error: unknown): {
  body: unknown
  statusCode: number
} {
  if (!(error instanceof IpcClientError)) {
    return { statusCode: 500, body: { error: INTERNAL_SERVER_ERROR_MESSAGE } }
  }

  return {
    statusCode: 400,
    body: {
      error: error.code
        ? {
            code: error.code,
            ...(error.details === undefined ? {} : { details: error.details }),
          }
        : error.message,
    },
  }
}

/** Request metadata made available to request wrappers and lifecycle hooks. */
type RequestHookInput = {
  name: string
  payload: unknown
  request: Request
}

/** Lifecycle data passed to request-received hooks. */
type RequestReceivedHookInput = RequestHookInput

/** Lifecycle data passed to request-response hooks. */
type ResponseSentHookInput = RequestHookInput & {
  response: unknown
  durationMs: number
}

/** Lifecycle data passed to request-failed hooks. */
type RequestFailedHookInput = RequestHookInput & {
  error: unknown
  durationMs: number
}

/** Wraps one request lifecycle so callers can install ambient async context around handlers and hooks. */
type RunHandlerHook = <T>(input: RequestHookInput, handler: () => Promise<T> | T) => Promise<T> | T

/** Optional hooks that run during request handling. */
type CreateServerConfig<TRoutes extends HttpRouteTree> = {
  port: number
  hostname?: string
  routes: TRoutes
  handlers: RouteRequestHandlerMap<TRoutes>
  browserAccess?: {
    readonly allowedOrigins: readonly string[]
    readonly isAllowedOrigin?: (origin: string) => boolean
    readonly authorizeRequest?: (
      request: Request,
    ) => Promise<Response | null | undefined> | Response | null | undefined
  }
  runHandler?: RunHandlerHook
  onRequestReceived?: (input: RequestReceivedHookInput) => Promise<void> | void
  onResponseSent?: (input: ResponseSentHookInput) => Promise<void> | void
  onRequestFailed?: (input: RequestFailedHookInput) => Promise<void> | void
}

/** Creates the Node IPC server for one TCP-backed Rouzer route tree. */
export function createServer<TRoutes extends HttpRouteTree>(config: CreateServerConfig<TRoutes>) {
  const { port, hostname = "127.0.0.1", routes } = config
  const browserAccess = config.browserAccess
    ? {
        allowedOrigins: new Set(config.browserAccess.allowedOrigins.map(normalizeAllowedOrigin)),
        isAllowedOrigin: config.browserAccess.isAllowedOrigin,
        authorizeRequest: config.browserAccess.authorizeRequest,
      }
    : null
  const handlers = wrapHandlers(routes, config.handlers, config)
  const router = createRouter({
    plugins: [ndjson.routerPlugin],
  }).use(routes, handlers as RouteRequestHandlerMap<TRoutes, any>)

  const server = http.createServer((req, res) => {
    void handleRequest(req, res)
  })

  async function handleRequest(req: http.IncomingMessage, res: http.ServerResponse) {
    const responseHeaders = new Headers()
    let webRequest: { readonly request: Request; readonly cleanup: () => void } | undefined
    try {
      if (browserAccess) {
        const host = validateBrowserAccessHost(req, server, hostname, port)
        if (!host.valid) {
          await writeResponse(res, forbiddenResponse())
          return
        }

        const browserHeaders = resolveBrowserAccessHeaders(req, browserAccess)
        if (req.method === "OPTIONS") {
          await writeResponse(
            res,
            browserHeaders
              ? new Response(null, { status: 204, headers: browserHeaders })
              : forbiddenResponse(),
          )
          return
        }

        if (hasOriginHeader(req) && !browserHeaders) {
          await writeResponse(res, forbiddenResponse())
          return
        }

        if (browserHeaders) {
          for (const [name, value] of browserHeaders) {
            responseHeaders.set(name, value)
          }
        }
      }

      webRequest = await createWebRequest(req, res, hostname, port)
      if (browserAccess && hasOriginHeader(req) && browserAccess.authorizeRequest) {
        const authorizationResponse = await browserAccess.authorizeRequest(webRequest.request)
        if (authorizationResponse) {
          await writeResponse(res, mergeResponseHeaders(authorizationResponse, responseHeaders))
          return
        }
      }

      const response = await router({
        request: webRequest.request,
        ip: req.socket.remoteAddress ?? "",
        platform: undefined,
        env: () => undefined,
        passThrough: () => {},
        waitUntil: (promise: Promise<unknown>) => {
          void promise
        },
        setHeader: (name: string, value: string) => {
          responseHeaders.set(name, value)
        },
      } as never)

      if (!response) {
        await writeResponse(res, new Response(null, { status: 404 }))
        return
      }

      await writeResponse(res, mergeResponseHeaders(response, responseHeaders))
    } catch (error) {
      if (res.headersSent) {
        res.destroy(error instanceof Error ? error : undefined)
        return
      }

      const { statusCode, body } = getErrorResponse(error)
      await writeResponse(res, Response.json(body, { status: statusCode }))
    } finally {
      webRequest?.cleanup()
    }
  }

  server.listen(port, hostname)

  return { server }
}

function forbiddenResponse() {
  return Response.json({ error: "Forbidden" }, { status: 403 })
}

function normalizeAllowedOrigin(origin: string) {
  if (origin === "*" || origin === "null") {
    throw new Error(`Browser access origin must be explicit: ${origin}`)
  }

  const url = new URL(origin)
  if (url.origin !== origin) {
    throw new Error(`Browser access origin must not include a path, query, or hash: ${origin}`)
  }

  return url.origin
}

function validateBrowserAccessHost(
  req: http.IncomingMessage,
  server: http.Server,
  hostname: string,
  configuredPort: number,
) {
  const host = req.headers.host
  if (!host) {
    return { valid: false }
  }

  const address = server.address()
  const port =
    address && typeof address !== "string"
      ? address.port
      : configuredPort === 0
        ? null
        : configuredPort
  if (port === null) {
    return { valid: false }
  }

  const expectedHosts = new Set([
    `${hostname}:${port}`,
    `127.0.0.1:${port}`,
    `localhost:${port}`,
    `[::1]:${port}`,
  ])

  return { valid: expectedHosts.has(host) }
}

function hasOriginHeader(req: http.IncomingMessage) {
  return req.headers.origin !== undefined
}

function resolveBrowserAccessHeaders(
  req: http.IncomingMessage,
  browserAccess: {
    readonly allowedOrigins: ReadonlySet<string>
    readonly isAllowedOrigin?: (origin: string) => boolean
  },
): Headers | null {
  const origin = req.headers.origin
  if (typeof origin !== "string") {
    return null
  }

  let normalizedOrigin: string
  try {
    normalizedOrigin = new URL(origin).origin
  } catch {
    return null
  }

  if (
    normalizedOrigin !== origin ||
    (!browserAccess.allowedOrigins.has(normalizedOrigin) &&
      browserAccess.isAllowedOrigin?.(normalizedOrigin) !== true)
  ) {
    return null
  }

  const headers = new Headers({
    "Access-Control-Allow-Origin": normalizedOrigin,
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "authorization, content-type",
    Vary: "Origin, Access-Control-Request-Private-Network",
  })

  if (req.headers["access-control-request-private-network"] === "true") {
    headers.set("Access-Control-Allow-Private-Network", "true")
  }

  return headers
}

function wrapHandlers(
  routes: HttpRouteTree,
  handlers: Record<string, unknown>,
  config: Pick<
    CreateServerConfig<HttpRouteTree>,
    "runHandler" | "onRequestReceived" | "onResponseSent" | "onRequestFailed"
  >,
  path: readonly string[] = [],
) {
  const wrappedHandlers: Record<string, unknown> = {}

  for (const [key, route] of Object.entries(routes)) {
    const handler = handlers[key]
    if (route.kind === "resource") {
      wrappedHandlers[key] =
        typeof handler === "object" && handler !== null
          ? wrapHandlers(route.children, handler as Record<string, unknown>, config, [...path, key])
          : handler
      continue
    }

    wrappedHandlers[key] =
      typeof handler === "function" && !isStreamRoute(route)
        ? wrapRequestHandler([...path, key].join("."), handler as (...args: any[]) => any, config)
        : handler
  }

  return wrappedHandlers
}

function wrapRequestHandler(
  name: string,
  handler: (...args: any[]) => any,
  config: Pick<
    CreateServerConfig<HttpRouteTree>,
    "runHandler" | "onRequestReceived" | "onResponseSent" | "onRequestFailed"
  >,
) {
  return async (context: { body?: unknown; query?: unknown; request: Request }) => {
    const startedAt = Date.now()
    const requestInput: RequestHookInput = {
      name,
      payload: "body" in context ? context.body : context.query,
      request: context.request,
    }

    const processRequest = async () => {
      try {
        await config.onRequestReceived?.(requestInput)
        const response = await handler(context)
        await config.onResponseSent?.({
          ...requestInput,
          response,
          durationMs: Date.now() - startedAt,
        })
        return response
      } catch (error) {
        await config.onRequestFailed?.({
          ...requestInput,
          error,
          durationMs: Date.now() - startedAt,
        })
        throw error
      }
    }

    return await (config.runHandler
      ? config.runHandler(requestInput, processRequest)
      : processRequest())
  }
}

function isStreamRoute(route: HttpAction) {
  return getResponsePluginMarkerId(route.schema.response) === "rouzer/ndjson"
}

async function createWebRequest(
  req: http.IncomingMessage,
  res: http.ServerResponse,
  hostname: string,
  port: number,
): Promise<{ request: Request; cleanup: () => void }> {
  const requestUrl = new URL(req.url ?? "/", `http://${req.headers.host ?? `${hostname}:${port}`}`)
  const abortController = new AbortController()
  const abortRequest = () => {
    abortController.abort()
  }
  res.on("close", abortRequest)
  req.socket.on("close", abortRequest)

  const init: RequestInit = {
    method: req.method,
    headers: req.headers as HeadersInit,
    signal: abortController.signal,
  }

  if (req.method !== "GET" && req.method !== "HEAD") {
    init.body = await readBody(req)
  }

  return {
    request: new Request(requestUrl, init),
    cleanup: () => {
      res.off("close", abortRequest)
      req.socket.off("close", abortRequest)
    },
  }
}

/** Reads one IPC request body into a UTF-8 string payload. */
async function readBody(req: http.IncomingMessage) {
  let body = ""
  for await (const chunk of req) {
    body += chunk
  }
  return body
}

function mergeResponseHeaders(response: Response, extraHeaders: Headers) {
  for (const [name, value] of response.headers) {
    extraHeaders.set(name, value)
  }

  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers: extraHeaders,
  })
}

async function writeResponse(res: http.ServerResponse, response: Response) {
  res.writeHead(response.status, Object.fromEntries(response.headers.entries()))
  res.flushHeaders()

  if (!response.body) {
    res.end()
    return
  }

  const reader = response.body.getReader()
  const cancelBody = () => {
    void reader.cancel().catch(() => {})
  }
  res.on("close", cancelBody)
  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        break
      }
      if (res.destroyed || res.writableEnded) {
        return
      }
      res.write(Buffer.from(value))
    }
    if (!res.destroyed && !res.writableEnded) {
      res.end()
    }
  } catch (error) {
    if (!res.destroyed) {
      res.destroy(error instanceof Error ? error : undefined)
    }
  } finally {
    res.off("close", cancelBody)
    reader.releaseLock()
  }
}

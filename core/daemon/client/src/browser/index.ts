/** Browser-safe daemon IPC client helpers built on Fetch and explicit bearer tokens. */
import { createRouteClient, IpcClientError, ndjson, type IpcClientHook } from "@goddard-ai/ipc"

import { daemonIpcRoutes } from "../daemon-ipc.ts"
import type { DaemonIpcClient } from "../index.ts"

export type { DaemonIpcClient } from "../index.ts"

/** Authorization callback used by the browser transport before each daemon request. */
export type BrowserDaemonTokenProvider = () =>
  | Promise<string | null | undefined>
  | string
  | null
  | undefined

/** Error raised when a browser daemon request cannot reach the loopback daemon. */
export class BrowserDaemonConnectionError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = "BrowserDaemonConnectionError"
  }
}

/** Error raised when the daemon rejects browser-origin authorization. */
export class BrowserDaemonAuthorizationError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message)
    this.name = "BrowserDaemonAuthorizationError"
  }
}

/** Creates one browser-safe daemon IPC client from a loopback URL and bearer-token source. */
export function createBrowserDaemonIpcClient(options: {
  daemonUrl: string
  token?: BrowserDaemonTokenProvider
  ipcHook?: IpcClientHook
  fetch?: typeof fetch
}): DaemonIpcClient {
  const daemonUrl = options.daemonUrl
  const fetchImpl = options.fetch ?? fetch

  return createRouteClient({
    baseURL: daemonUrl,
    routes: daemonIpcRoutes,
    plugins: [ndjson.clientPlugin],
    clientHook: options.ipcHook,
    fetch: (async (input, init) => {
      const headers = new Headers(init?.headers)
      const token = await options.token?.()
      if (token) {
        headers.set("Authorization", `Bearer ${token}`)
      }

      try {
        return await fetchImpl(input, {
          ...init,
          headers,
        })
      } catch (error) {
        throw new BrowserDaemonConnectionError(
          `Could not connect to Goddard daemon at ${daemonUrl}.`,
          { cause: error },
        )
      }
    }) as typeof fetch,
    onJsonError: async (response) => {
      const body = (await response.json().catch(() => undefined)) as
        | {
            error?: unknown
            message?: unknown
          }
        | undefined
      if (isStructuredIpcError(body?.error)) {
        throw new IpcClientError({
          code: body.error.code,
          details: body.error.details,
        })
      }
      if (response.status === 401 || response.status === 403) {
        throw new BrowserDaemonAuthorizationError(
          resolveErrorMessage(body, response),
          response.status,
        )
      }

      throw new Error(resolveErrorMessage(body, response))
    },
  }) as DaemonIpcClient
}

function isStructuredIpcError(value: unknown): value is {
  code: string
  details?: unknown
} {
  return (
    typeof value === "object" && value !== null && "code" in value && typeof value.code === "string"
  )
}

function resolveErrorMessage(
  body:
    | {
        error?: unknown
        message?: unknown
      }
    | undefined,
  response: Response,
) {
  if (typeof body?.error === "string") {
    return body.error
  }
  if (typeof body?.message === "string") {
    return body.message
  }
  return `Daemon request failed with status ${response.status}`
}

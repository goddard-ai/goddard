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

/** Browser daemon endpoint and bearer credentials resolved by a browser host. */
export type BrowserDaemonAccess = {
  daemonUrl: string
  token?: string | null | undefined
}

/** Access callback used when the browser host must resolve daemon access lazily. */
export type BrowserDaemonAccessProvider = () => Promise<BrowserDaemonAccess> | BrowserDaemonAccess
type BrowserFetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>

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

export type BrowserDaemonIpcClientOptions = {
  ipcHook?: IpcClientHook
  fetch?: BrowserFetch
} & (
  | {
      daemonUrl: string
      token?: BrowserDaemonTokenProvider
    }
  | {
      access: BrowserDaemonAccessProvider
    }
)

/** Creates one browser-safe daemon IPC client from a loopback URL and bearer-token source. */
export function createBrowserDaemonIpcClient(
  options: BrowserDaemonIpcClientOptions & {
    daemonUrl: string
  },
): DaemonIpcClient

export function createBrowserDaemonIpcClient(
  options: BrowserDaemonIpcClientOptions & {
    access: BrowserDaemonAccessProvider
  },
): DaemonIpcClient

export function createBrowserDaemonIpcClient(
  options: BrowserDaemonIpcClientOptions,
): DaemonIpcClient {
  const fetchImpl = options.fetch ?? globalThis.fetch

  return createRouteClient({
    baseURL: "http://goddard-daemon.local",
    routes: daemonIpcRoutes,
    plugins: [ndjson.clientPlugin],
    clientHook: options.ipcHook,
    fetch: (async (input, init) => {
      const request = async () => {
        const { origin, token } = await resolveBrowserDaemonAccess(options)
        let url = new URL(input instanceof Request ? input.url : String(input))
        url = new URL(url.pathname + url.search, origin)

        const headers = new Headers(init?.headers)
        if (token) {
          headers.set("Authorization", `Bearer ${token}`)
        }

        try {
          return await fetchImpl(url, {
            ...init,
            headers,
          })
        } catch (error) {
          throw new BrowserDaemonConnectionError(
            `Could not connect to Goddard daemon at ${origin}.`,
            { cause: error },
          )
        }
      }

      const response = await request()
      if ("access" in options && isAuthorizationDenied(response)) {
        return await request()
      }

      return response
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
      if (isAuthorizationDenied(response)) {
        throw new BrowserDaemonAuthorizationError(
          resolveErrorMessage(body, response),
          response.status,
        )
      }

      throw new Error(resolveErrorMessage(body, response))
    },
  })
}

async function resolveBrowserDaemonAccess(options: BrowserDaemonIpcClientOptions) {
  let origin: string
  let token: string | null | undefined
  if ("access" in options) {
    const access = await options.access()
    origin = access.daemonUrl
    token = access.token
  } else {
    origin = options.daemonUrl
    token = "token" in options ? await options.token?.() : undefined
  }

  return { origin, token }
}

function isAuthorizationDenied(response: Response) {
  return response.status === 401 || response.status === 403
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

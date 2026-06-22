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
  fetch?: typeof globalThis.fetch
}): DaemonIpcClient
export function createBrowserDaemonIpcClient(options: {
  access: BrowserDaemonAccessProvider
  ipcHook?: IpcClientHook
  fetch?: typeof globalThis.fetch
}): DaemonIpcClient
export function createBrowserDaemonIpcClient(
  options:
    | {
        daemonUrl: string
        token?: BrowserDaemonTokenProvider
        ipcHook?: IpcClientHook
        fetch?: typeof globalThis.fetch
      }
    | {
        access: BrowserDaemonAccessProvider
        ipcHook?: IpcClientHook
        fetch?: typeof globalThis.fetch
      },
): DaemonIpcClient {
  const fetchImpl = options.fetch ?? globalThis.fetch
  let accessPromise: Promise<BrowserDaemonAccess> | undefined

  return createRouteClient({
    baseURL: "http://goddard-daemon.local",
    routes: daemonIpcRoutes,
    plugins: [ndjson.clientPlugin],
    clientHook: options.ipcHook,
    fetch: (async (input, init) => {
      const response = await fetchWithAccess({ input, init, options, fetchImpl, getAccess })
      if (
        "access" in options &&
        (response.status === 401 || response.status === 403) &&
        accessPromise
      ) {
        accessPromise = undefined
        return await fetchWithAccess({ input, init, options, fetchImpl, getAccess })
      }

      return response
    }) as typeof globalThis.fetch,
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
  })

  function getAccess() {
    if ("access" in options) {
      accessPromise ??= Promise.resolve(options.access())
      return accessPromise
    }

    return Promise.resolve({
      daemonUrl: options.daemonUrl,
    })
  }
}

async function fetchWithAccess(input: {
  input: RequestInfo | URL
  init: RequestInit | undefined
  options:
    | {
        daemonUrl: string
        token?: BrowserDaemonTokenProvider
      }
    | {
        access: BrowserDaemonAccessProvider
      }
  fetchImpl: typeof globalThis.fetch
  getAccess: () => Promise<BrowserDaemonAccess>
}) {
  const access = await input.getAccess()
  const headers = new Headers(input.init?.headers)
  const token = "access" in input.options ? access.token : await input.options.token?.()
  if (token) {
    headers.set("Authorization", `Bearer ${token}`)
  }

  try {
    return await input.fetchImpl(resolveDaemonRequestUrl(input.input, access.daemonUrl), {
      ...input.init,
      headers,
    })
  } catch (error) {
    throw new BrowserDaemonConnectionError(
      `Could not connect to Goddard daemon at ${access.daemonUrl}.`,
      { cause: error },
    )
  }
}

function resolveDaemonRequestUrl(input: RequestInfo | URL, daemonUrl: string) {
  const requestUrl = new URL(input instanceof Request ? input.url : String(input))
  return new URL(`${requestUrl.pathname}${requestUrl.search}`, daemonUrl).toString()
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

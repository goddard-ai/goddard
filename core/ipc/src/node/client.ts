import { createClient, type RouzerClientHook } from "rouzer"
import type { HttpRouteTree } from "rouzer/http"
import * as ndjson from "rouzer/ndjson"

import { IpcClientError } from "../errors.ts"

/** TCP address used by the Node IPC transport. */
export type NodeTcpAddress = {
  hostname: string
  port: number
}

/** Rewords low-level TCP connection failures with the requested IPC server address. */
function toTcpConnectionError(error: unknown, address: NodeTcpAddress) {
  if (!(error instanceof Error)) {
    return error
  }

  const errorCode =
    (error as Error & { code?: unknown }).code ??
    (error.cause as (Error & { code?: unknown }) | undefined)?.code
  if (
    errorCode !== "ECONNREFUSED" &&
    errorCode !== "EHOSTUNREACH" &&
    errorCode !== "ENOTFOUND" &&
    !error.message.includes("Unable to connect")
  ) {
    return error
  }

  return new IpcClientError(
    `Could not connect to IPC server at ${formatAddress(address)}. ` +
      "The server may not be running, or the TCP address may be wrong.",
    {
      cause: error,
    },
  )
}

/** Creates the typed IPC client backed by Rouzer over Node fetch. */
export function createNodeClient<const TRoutes extends HttpRouteTree>(
  address: NodeTcpAddress,
  routes: TRoutes,
  options: { ipcHook?: RouzerClientHook } = {},
) {
  return createClient({
    baseURL: formatAddress(address),
    routes,
    plugins: [ndjson.clientPlugin],
    clientHook: options.ipcHook,
    fetch: (async (input, init) => {
      try {
        return await fetch(input, init)
      } catch (error) {
        throw toTcpConnectionError(error, address)
      }
    }) as typeof fetch,
    onJsonError: async (response) => {
      const body = (await response.json().catch(() => undefined)) as
        | {
            error?:
              | unknown
              | {
                  code?: unknown
                  details?: unknown
                  message?: unknown
                }
            message?: unknown
          }
        | undefined
      if (isStructuredIpcError(body?.error)) {
        throw new IpcClientError({
          code: body.error.code,
          details: body.error.details,
          message: body.error.message,
        })
      }

      const message =
        typeof body?.error === "string"
          ? body.error
          : typeof body?.message === "string"
            ? body.message
            : `IPC request failed with status ${response.status}`
      throw new Error(message)
    },
  })
}

function isStructuredIpcError(value: unknown): value is {
  code: string
  details?: unknown
  message: string
} {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value &&
    typeof value.code === "string" &&
    typeof value.message === "string"
  )
}

function formatAddress(address: NodeTcpAddress) {
  const url = new URL("http://localhost")
  url.hostname = address.hostname
  url.port = String(address.port)
  return url.toString()
}

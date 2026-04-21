import * as http from "node:http"

import { createClient } from "../client.ts"
import { IpcClientError } from "../errors.ts"
import { type IpcSchema } from "../schema.ts"
import { type IpcTransport } from "../transport.ts"

/** Normalizes one failed IPC response body into a human-readable error message. */
function getErrorMessage(body: string): string {
  if (!body) {
    return "IPC request failed"
  }

  try {
    const parsed = JSON.parse(body) as { error?: unknown }
    if (typeof parsed.error === "string") {
      return parsed.error
    }
  } catch {
    // Keep the raw body if it is not JSON.
  }

  return body
}

/** Rewords low-level socket connection failures with the requested IPC socket path. */
function toSocketConnectionError(error: unknown, socketPath: string) {
  if (!(error instanceof Error)) {
    return error
  }

  const errorCode = (error as Error & { code?: unknown }).code
  if (
    errorCode !== "FailedToOpenSocket" &&
    errorCode !== "ENOENT" &&
    errorCode !== "ECONNREFUSED"
  ) {
    return error
  }

  return new IpcClientError(
    `Could not connect to IPC endpoint at ${socketPath}. ` +
      "The server may not be running, or the endpoint may be wrong.",
    {
      cause: error,
    },
  )
}

/** Builds one HTTP request target for either a socket-backed or loopback IPC endpoint. */
function getRequestOptions(
  socketPath: string,
  requestPath: string,
  method: "GET" | "POST",
  headers?: http.OutgoingHttpHeaders,
) {
  const networkOrigin = readNetworkOrigin(socketPath)
  if (!networkOrigin) {
    return {
      socketPath,
      path: requestPath,
      method,
      headers,
    }
  }

  // The daemon still threads this value through the historical `socketPath` field because higher
  // layers encode one "local daemon endpoint" there, even when Windows falls back to loopback HTTP.
  const url = new URL(requestPath, networkOrigin)
  return {
    hostname: url.hostname,
    port: url.port ? Number.parseInt(url.port, 10) : undefined,
    path: `${url.pathname}${url.search}`,
    method,
    headers,
  }
}

/** Creates the Node HTTP-over-socket transport for one daemon socket path. */
export function createNodeTransport(socketPath: string): IpcTransport {
  async function send(name: string, payload: unknown): Promise<unknown> {
    const wireData = JSON.stringify({ name, payload })
    return new Promise((resolve, reject) => {
      const req = http.request(
        getRequestOptions(socketPath, "/", "POST", {
          "Content-Type": "application/json",
          "Content-Length": Buffer.byteLength(wireData),
        }),
        (res: http.IncomingMessage) => {
          let body = ""
          res.setEncoding("utf8")
          res.on("data", (chunk: string) => {
            body += chunk
          })
          res.on("end", () => {
            if (res.statusCode !== 200) {
              reject(new Error(getErrorMessage(body)))
              return
            }

            try {
              resolve(JSON.parse(body))
            } catch (error) {
              reject(error)
            }
          })
        },
      )

      req.on("error", (error: unknown) => {
        reject(toSocketConnectionError(error, socketPath))
      })
      req.write(wireData)
      req.end()
    })
  }

  async function subscribe(
    name: string,
    filter: unknown,
    onMessage: (payload: unknown) => void,
  ): Promise<() => void> {
    return await new Promise((resolve, reject) => {
      let settled = false
      let response: http.IncomingMessage | undefined
      let errorBody = ""

      const req = http.request(
        getRequestOptions(
          socketPath,
          `/stream?name=${encodeURIComponent(name)}${
            filter === undefined ? "" : `&filter=${encodeURIComponent(JSON.stringify(filter))}`
          }`,
          "GET",
        ),
        (res: http.IncomingMessage) => {
          response = res

          if (res.statusCode !== 200) {
            res.setEncoding("utf8")
            res.on("data", (chunk: string) => {
              errorBody += chunk
            })
            res.on("end", () => {
              if (!settled) {
                settled = true
                reject(new Error(getErrorMessage(errorBody)))
              }
            })
            return
          }

          settled = true
          resolve(() => {
            if (response && !response.destroyed) {
              response.destroy()
            }
            req.destroy()
          })

          let buffer = ""
          res.setEncoding("utf8")
          res.on("data", (chunk: string) => {
            buffer += chunk
            const lines = buffer.split("\n")
            buffer = lines.pop() ?? ""

            for (const line of lines) {
              if (!line.trim()) {
                continue
              }

              const message = JSON.parse(line) as { name?: unknown; payload?: unknown }
              if (message.name === name) {
                onMessage(message.payload)
              }
            }
          })
        },
      )

      req.on("error", (error: unknown) => {
        if (!settled) {
          settled = true
          reject(toSocketConnectionError(error, socketPath))
        }
      })
      req.end()
    })
  }

  return { send, subscribe }
}

/** Parses one IPC target into a loopback URL when the daemon is using network transport. */
function readNetworkOrigin(socketPath: string) {
  try {
    const url = new URL(socketPath)
    return url.protocol === "http:" ? url : null
  } catch {
    return null
  }
}

/** Creates the typed IPC client backed by the Node socket transport. */
export function createNodeClient<S extends IpcSchema>(socketPath: string, schema: S) {
  return createClient(schema, createNodeTransport(socketPath))
}

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
    `Could not connect to IPC socket at ${socketPath}. ` +
      "The server may not be running, or the socket path may be wrong.",
    {
      cause: error,
    },
  )
}

/** Creates the Node HTTP-over-socket transport for one daemon socket path. */
export function createNodeTransport(socketPath: string): IpcTransport {
  async function send(name: string, payload: unknown): Promise<unknown> {
    const wireData = JSON.stringify({ name, payload })
    return new Promise((resolve, reject) => {
      const req = http.request(
        {
          socketPath,
          path: "/",
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            "Content-Length": Buffer.byteLength(wireData),
          },
        },
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
        {
          socketPath,
          path: `/stream?name=${encodeURIComponent(name)}${
            filter === undefined ? "" : `&filter=${encodeURIComponent(JSON.stringify(filter))}`
          }`,
          method: "GET",
        },
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

/** Creates the typed IPC client backed by the Node socket transport. */
export function createNodeClient<S extends IpcSchema>(socketPath: string, schema: S) {
  return createClient(schema, createNodeTransport(socketPath))
}

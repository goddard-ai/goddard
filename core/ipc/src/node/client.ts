import * as http from "node:http"

import { createClient } from "../client.ts"
import { IpcClientError } from "../errors.ts"
import { type IpcSchema } from "../schema.ts"
import { type IpcTransport } from "../transport.ts"

/** TCP address used by the Node IPC transport. */
export type NodeTcpAddress = {
  hostname: string
  port: number
}

/** Normalizes one failed IPC response body into a human-readable error message. */
function getErrorMessage(body: string) {
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

/** Rewords low-level TCP connection failures with the requested IPC server address. */
function toTcpConnectionError(error: unknown, address: NodeTcpAddress) {
  if (!(error instanceof Error)) {
    return error
  }

  const errorCode = (error as Error & { code?: unknown }).code
  if (errorCode !== "ECONNREFUSED" && errorCode !== "EHOSTUNREACH" && errorCode !== "ENOTFOUND") {
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

/** Creates the Node HTTP-over-TCP transport for one daemon address. */
export function createNodeTransport(address: NodeTcpAddress): IpcTransport {
  async function send(name: string, payload: unknown): Promise<unknown> {
    const wireData = JSON.stringify({ name, payload })
    return new Promise((resolve, reject) => {
      const req = http.request(
        {
          hostname: address.hostname,
          port: address.port,
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
        reject(toTcpConnectionError(error, address))
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
          hostname: address.hostname,
          port: address.port,
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

              const message = JSON.parse(line) as {
                name?: unknown
                payload?: unknown
              }
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
          reject(toTcpConnectionError(error, address))
        }
      })
      req.end()
    })
  }

  return { send, subscribe }
}

/** Creates the typed IPC client backed by the Node TCP transport. */
export function createNodeClient<S extends IpcSchema>(address: NodeTcpAddress, schema: S) {
  return createClient(schema, createNodeTransport(address))
}

function formatAddress(address: NodeTcpAddress) {
  const url = new URL("http://localhost")
  url.hostname = address.hostname
  url.port = String(address.port)
  return url.toString()
}

import * as http from "node:http"
import { createClient } from "./client.ts"
import { type AppSchema } from "./schema.ts"
import { type IpcTransport } from "./transport.ts"

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

/**
 * Creates an IPC transport for Node.js over Unix domain sockets.
 *
 * @param socketPath - The path to the Unix domain socket.
 * @returns An `IpcTransport` implementation.
 */
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
        (res) => {
          let body = ""
          res.setEncoding("utf8")
          res.on("data", (chunk) => {
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

      req.on("error", reject)
      req.write(wireData)
      req.end()
    })
  }

  async function subscribe(
    name: string,
    onMessage: (payload: unknown) => void,
  ): Promise<() => void> {
    return await new Promise((resolve, reject) => {
      let settled = false
      let response: http.IncomingMessage | undefined
      let errorBody = ""

      const req = http.request(
        {
          socketPath,
          path: `/stream?name=${encodeURIComponent(name)}`,
          method: "GET",
        },
        (res) => {
          response = res

          if (res.statusCode !== 200) {
            res.setEncoding("utf8")
            res.on("data", (chunk) => {
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
          res.on("data", (chunk) => {
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

      req.on("error", (error) => {
        if (!settled) {
          settled = true
          reject(error)
        }
      })
      req.end()
    })
  }

  return { send, subscribe }
}

/**
 * Creates a complete IPC client for Node.js connecting to the given socket.
 *
 * @param socketPath - The path to the Unix domain socket.
 * @param schema - The IPC application schema defining the valid requests and streams.
 * @returns An object with strongly-typed `send` and `subscribe` methods.
 */
export function createNodeClient<S extends AppSchema>(socketPath: string, schema: S) {
  return createClient(schema, createNodeTransport(socketPath))
}

import { existsSync, unlinkSync } from "node:fs"
import * as http from "node:http"
import {
  type AppSchema,
  type ReqName,
  type ReqPayload,
  type ResType,
  type StrName,
  type StrPayload,
} from "./schema.ts"

export type Handlers<S extends AppSchema> = {
  [K in ReqName<S>]: (payload: ReqPayload<S, K>) => Promise<ResType<S, K>> | ResType<S, K>
}

type StreamClient = {
  name: string
  res: http.ServerResponse
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function sendJson(res: http.ServerResponse, statusCode: number, body: unknown) {
  res.writeHead(statusCode, { "Content-Type": "application/json" })
  res.end(JSON.stringify(body))
}

async function readBody(req: http.IncomingMessage): Promise<string> {
  let body = ""
  for await (const chunk of req) {
    body += chunk
  }
  return body
}

function safeUnlink(socketPath: string) {
  if (!existsSync(socketPath)) {
    return
  }

  try {
    unlinkSync(socketPath)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
      throw error
    }
  }
}

export function createServer<S extends AppSchema>(
  socketPath: string,
  schema: S,
  handlers: Handlers<S>,
) {
  const streamClients = new Set<StreamClient>()

  function publish<K extends StrName<S>>(name: K, payload: StrPayload<S, K>) {
    const validPayload = schema.server.streams[name].parse(payload)
    const chunk = JSON.stringify({ name, payload: validPayload }) + "\n"

    for (const client of streamClients) {
      if (client.name === name && !client.res.destroyed && !client.res.writableEnded) {
        client.res.write(chunk)
      }
    }
  }

  const server: http.Server = http.createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost")

    if (req.method === "POST" && url.pathname === "/") {
      void (async () => {
        try {
          const body = await readBody(req)
          const message = JSON.parse(body) as { name?: unknown; payload?: unknown }

          if (typeof message.name !== "string") {
            throw new Error("Request name must be a string")
          }

          const routeDef = schema.client.requests[message.name]
          if (!routeDef) {
            throw new Error(`Unknown request: ${message.name}`)
          }

          const validPayload = routeDef.payload.parse(message.payload)
          const handler = handlers[message.name as ReqName<S>] as (
            payload: typeof validPayload,
          ) => unknown
          const responseData = await handler(validPayload)

          sendJson(res, 200, responseData)
        } catch (error) {
          sendJson(res, 400, { error: getErrorMessage(error) })
        }
      })()
      return
    }

    if (req.method === "GET" && url.pathname === "/stream") {
      const name = url.searchParams.get("name")
      if (!name || !Object.hasOwn(schema.server.streams, name)) {
        res.writeHead(400, { "Content-Type": "text/plain" })
        res.end("Invalid stream name")
        return
      }

      res.writeHead(200, {
        "Content-Type": "application/x-ndjson",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      })
      res.flushHeaders()

      const client = { name, res }
      streamClients.add(client)

      const removeClient = () => {
        streamClients.delete(client)
      }

      req.on("close", removeClient)
      res.on("close", removeClient)
      return
    }

    res.writeHead(404)
    res.end()
  })

  safeUnlink(socketPath)
  server.listen(socketPath)
  server.on("close", () => {
    safeUnlink(socketPath)
  })

  return { server, publish }
}
